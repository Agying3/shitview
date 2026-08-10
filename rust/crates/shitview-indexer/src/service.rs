use crossbeam_channel::{bounded, select, Receiver, Sender};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use shitview_core::{IndexRecord, NodeKind, ScanIssue};
use shitview_storage::{Generation, IndexStore, PendingDirectory, StoredNode};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub worker_count: usize,
    pub queue_capacity: usize,
    pub visible_node_limit: usize,
    pub watch: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        let workers = thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(4)
            .clamp(2, 16);
        Self {
            worker_count: workers,
            queue_capacity: workers * 4,
            visible_node_limit: 10_000,
            watch: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexPhase {
    Starting,
    Scanning,
    Paused,
    ReplayingChanges,
    Watching,
    Complete,
    CompleteWithWarnings,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexProgress {
    pub phase: IndexPhase,
    pub generation: i64,
    pub indexed_nodes: usize,
    pub pending_directories: usize,
    pub issue_count: usize,
    pub resumed: bool,
}

#[derive(Debug, Clone)]
pub enum IndexEvent {
    Progress(IndexProgress),
    Nodes(Vec<StoredNode>),
    Warning(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum IndexCommand {
    Pause,
    Resume,
    Cancel,
    Prioritize(PathBuf),
    Shutdown,
}

pub struct IndexHandle {
    commands: Sender<IndexCommand>,
    events: Receiver<IndexEvent>,
    join: Option<JoinHandle<()>>,
}

impl IndexHandle {
    pub fn start(
        root: impl Into<PathBuf>,
        database_path: impl Into<PathBuf>,
        options: IndexOptions,
    ) -> io::Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a directory: {}", root.display()),
            ));
        }
        let database_path = database_path.into();
        let (command_tx, command_rx) = bounded(64);
        let (event_tx, event_rx) = bounded(256);
        let join = thread::Builder::new()
            .name("shitview-index-coordinator".to_owned())
            .spawn(move || {
                if let Err(error) = run_service(root, database_path, options, command_rx, &event_tx)
                {
                    let _ = event_tx.send(IndexEvent::Failed(error));
                }
            })?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            join: Some(join),
        })
    }

    pub fn events(&self) -> Receiver<IndexEvent> {
        self.events.clone()
    }

    pub fn send(&self, command: IndexCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "index service has stopped".to_owned())
    }

    pub fn pause(&self) -> Result<(), String> {
        self.send(IndexCommand::Pause)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.send(IndexCommand::Resume)
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.send(IndexCommand::Cancel)
    }

    pub fn prioritize(&self, path: impl Into<PathBuf>) -> Result<(), String> {
        self.send(IndexCommand::Prioritize(path.into()))
    }
}

impl Drop for IndexHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(IndexCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn default_database_path(root: &Path) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    let key = path_key(root);
    base.join("shitview")
        .join("indexes")
        .join(format!("{:016x}.sqlite3", fnv1a64(&key)))
}

#[derive(Debug, Clone)]
struct DirectoryTask {
    path: PathBuf,
    path_key: Vec<u8>,
    depth: usize,
    priority: i64,
}

#[derive(Debug)]
struct DirectoryResult {
    task: DirectoryTask,
    records: Vec<IndexRecord>,
    directories: Vec<DirectoryTask>,
    issues: Vec<ScanIssue>,
}

#[derive(Debug)]
struct BufferedWatchEvent {
    database_id: i64,
    event: Event,
}

fn run_service(
    root: PathBuf,
    database_path: PathBuf,
    options: IndexOptions,
    commands: Receiver<IndexCommand>,
    events: &Sender<IndexEvent>,
) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot open project root: {error}"))?;
    let root_display = display_path(&root);
    let project_key = project_key(&root);
    let mut store = IndexStore::open(&database_path)
        .map_err(|error| format!("cannot open index database: {error}"))?;
    let generation = store
        .begin_or_resume(&project_key, &root_display, now_ms())
        .map_err(|error| format!("cannot start index generation: {error}"))?;
    emit_progress(
        events,
        IndexPhase::Starting,
        &generation,
        &store,
        generation.resumed,
    );

    let (watch_tx, watch_rx) = bounded::<notify::Result<Event>>(options.queue_capacity.max(128));
    let mut watcher = if options.watch {
        let callback_tx = watch_tx.clone();
        match RecommendedWatcher::new(
            move |result| {
                let _ = callback_tx.try_send(result);
            },
            Config::default(),
        ) {
            Ok(mut watcher) => match watcher.watch(&root, RecursiveMode::Recursive) {
                Ok(()) => Some(watcher),
                Err(error) => {
                    let _ = events.send(IndexEvent::Warning(format!(
                        "file watcher could not start: {error}"
                    )));
                    None
                }
            },
            Err(error) => {
                let _ = events.send(IndexEvent::Warning(format!(
                    "file watcher could not be created: {error}"
                )));
                None
            }
        }
    } else {
        None
    };

    let scan_outcome = run_scan(
        &root,
        &generation,
        &mut store,
        &options,
        &commands,
        &watch_rx,
        events,
    )?;

    match scan_outcome {
        ScanOutcome::Cancelled => {
            store
                .cancel_generation(&generation, now_ms())
                .map_err(|error| format!("cannot cancel generation: {error}"))?;
            emit_progress(
                events,
                IndexPhase::Cancelled,
                &generation,
                &store,
                generation.resumed,
            );
            return Ok(());
        }
        ScanOutcome::Shutdown => {
            store
                .pause_generation(&generation, now_ms())
                .map_err(|error| format!("cannot pause generation: {error}"))?;
            return Ok(());
        }
        ScanOutcome::Complete(buffered) => {
            store
                .complete_generation(&generation, now_ms())
                .map_err(|error| format!("cannot switch index generation: {error}"))?;
            emit_progress(
                events,
                IndexPhase::ReplayingChanges,
                &generation,
                &store,
                generation.resumed,
            );
            for buffered_event in buffered {
                apply_watch_event(
                    &root,
                    &generation,
                    &mut store,
                    &buffered_event.event,
                    events,
                )?;
                let _ = store.mark_watch_event_applied(buffered_event.database_id);
            }
        }
    }

    publish_current_nodes(&store, &generation, options.visible_node_limit, events);
    let (_, issues, _) = store.counts(&generation).unwrap_or_default();
    emit_progress(
        events,
        if issues == 0 {
            IndexPhase::Complete
        } else {
            IndexPhase::CompleteWithWarnings
        },
        &generation,
        &store,
        generation.resumed,
    );

    if watcher.is_none() {
        return Ok(());
    }
    emit_progress(
        events,
        IndexPhase::Watching,
        &generation,
        &store,
        generation.resumed,
    );
    loop {
        select! {
            recv(commands) -> command => match command {
                Ok(IndexCommand::Shutdown | IndexCommand::Cancel) | Err(_) => break,
                Ok(IndexCommand::Pause) => {
                    watcher.take();
                    emit_progress(events, IndexPhase::Paused, &generation, &store, generation.resumed);
                }
                Ok(IndexCommand::Resume) if watcher.is_none() => {
                    let callback_tx = watch_tx.clone();
                    let mut resumed = RecommendedWatcher::new(
                        move |result| { let _ = callback_tx.try_send(result); },
                        Config::default(),
                    ).map_err(|error| format!("cannot resume watcher: {error}"))?;
                    resumed.watch(&root, RecursiveMode::Recursive)
                        .map_err(|error| format!("cannot watch project root: {error}"))?;
                    watcher = Some(resumed);
                    emit_progress(events, IndexPhase::Watching, &generation, &store, generation.resumed);
                }
                Ok(IndexCommand::Prioritize(_)) | Ok(IndexCommand::Resume) => {}
            },
            recv(watch_rx) -> watched => match watched {
                Ok(Ok(event)) => {
                    apply_watch_event(&root, &generation, &mut store, &event, events)?;
                    publish_current_nodes(&store, &generation, options.visible_node_limit, events);
                }
                Ok(Err(error)) => {
                    let _ = events.send(IndexEvent::Warning(format!("file watcher error: {error}")));
                }
                Err(_) => break,
            }
        }
    }
    Ok(())
}

enum ScanOutcome {
    Complete(Vec<BufferedWatchEvent>),
    Cancelled,
    Shutdown,
}

fn run_scan(
    root: &Path,
    generation: &Generation,
    store: &mut IndexStore,
    options: &IndexOptions,
    commands: &Receiver<IndexCommand>,
    watch_events: &Receiver<notify::Result<Event>>,
    events: &Sender<IndexEvent>,
) -> Result<ScanOutcome, String> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let (task_tx, task_rx) = bounded::<DirectoryTask>(options.queue_capacity.max(1));
    let (result_tx, result_rx) = bounded::<DirectoryResult>(options.queue_capacity.max(1));
    let mut workers = Vec::with_capacity(options.worker_count);
    for worker_index in 0..options.worker_count.max(1) {
        let task_rx = task_rx.clone();
        let result_tx = result_tx.clone();
        let root = root.to_path_buf();
        let cancelled = Arc::clone(&cancelled);
        workers.push(
            thread::Builder::new()
                .name(format!("shitview-scan-{worker_index}"))
                .spawn(move || {
                    while let Ok(task) = task_rx.recv() {
                        if cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        let result = scan_directory(&root, task, &cancelled);
                        if result_tx.send(result).is_err() {
                            break;
                        }
                    }
                })
                .map_err(|error| format!("cannot start scan worker: {error}"))?,
        );
    }
    drop(result_tx);

    let persisted = store
        .pending_directories(generation)
        .map_err(|error| format!("cannot load resumable scan queue: {error}"))?;
    let mut high = VecDeque::new();
    let mut normal = VecDeque::new();
    let mut queued = HashSet::new();
    if persisted.is_empty() {
        let task = DirectoryTask {
            path: root.to_path_buf(),
            path_key: path_key(root),
            depth: 0,
            priority: 0,
        };
        store
            .enqueue_directories(generation, &[pending_from_task(&task)])
            .map_err(|error| format!("cannot seed scan queue: {error}"))?;
        queued.insert(task.path_key.clone());
        normal.push_back(task);
    } else {
        for directory in persisted {
            let task = DirectoryTask {
                path: PathBuf::from(&directory.display_path),
                path_key: directory.path_key,
                depth: directory.depth,
                priority: directory.priority,
            };
            queued.insert(task.path_key.clone());
            if task.priority > 0 {
                high.push_back(task);
            } else {
                normal.push_back(task);
            }
        }
    }

    let mut active = 0usize;
    let mut paused = false;
    let mut buffered_watch_events = Vec::new();
    let outcome: ScanOutcome;

    loop {
        while !paused && active < options.worker_count.max(1) {
            let task = high.pop_front().or_else(|| normal.pop_front());
            let Some(task) = task else { break };
            if task_tx.send(task).is_err() {
                break;
            }
            active += 1;
        }
        if active == 0 && high.is_empty() && normal.is_empty() {
            outcome = ScanOutcome::Complete(buffered_watch_events);
            break;
        }

        select! {
            recv(commands) -> command => match command {
                Ok(IndexCommand::Pause) => {
                    paused = true;
                    let _ = store.pause_generation(generation, now_ms());
                    emit_progress(events, IndexPhase::Paused, generation, store, generation.resumed);
                }
                Ok(IndexCommand::Resume) => {
                    paused = false;
                    emit_progress(events, IndexPhase::Scanning, generation, store, generation.resumed);
                }
                Ok(IndexCommand::Cancel) => {
                    cancelled.store(true, Ordering::Relaxed);
                    outcome = ScanOutcome::Cancelled;
                    break;
                }
                Ok(IndexCommand::Shutdown) | Err(_) => {
                    cancelled.store(true, Ordering::Relaxed);
                    outcome = ScanOutcome::Shutdown;
                    break;
                }
                Ok(IndexCommand::Prioritize(path)) => {
                    promote_in_memory(&path, &mut high, &mut normal);
                    let _ = store.promote_path(generation, &display_path(&path));
                }
            },
            recv(result_rx) -> result => match result {
                Ok(result) => {
                    active = active.saturating_sub(1);
                    queued.remove(&result.task.path_key);
                    let mut pending = Vec::new();
                    for mut directory in result.directories {
                        if queued.insert(directory.path_key.clone()) {
                            directory.priority = 0;
                            pending.push(pending_from_task(&directory));
                            normal.push_back(directory);
                        }
                    }
                    store.commit_directory(
                        generation,
                        &result.task.path_key,
                        &result.records,
                        &pending,
                        &result.issues,
                        now_ms(),
                    ).map_err(|error| format!("cannot commit scan batch: {error}"))?;
                    emit_progress(events, IndexPhase::Scanning, generation, store, generation.resumed);
                }
                Err(_) => {
                    outcome = ScanOutcome::Shutdown;
                    break;
                }
            },
            recv(watch_events) -> watched => match watched {
                Ok(Ok(event)) => {
                    let primary = event.paths.first().map(|path| display_path(path)).unwrap_or_default();
                    let secondary = event.paths.get(1).map(|path| display_path(path));
                    match store.record_watch_event(
                        generation,
                        event_kind_name(&event.kind),
                        &primary,
                        secondary.as_deref(),
                        now_ms(),
                    ) {
                        Ok(database_id) => buffered_watch_events.push(BufferedWatchEvent { database_id, event }),
                        Err(error) => {
                            let _ = events.send(IndexEvent::Warning(format!("cannot buffer file event: {error}")));
                        }
                    }
                }
                Ok(Err(error)) => {
                    let _ = events.send(IndexEvent::Warning(format!("file watcher error: {error}")));
                }
                Err(_) => {}
            }
        }
    }

    cancelled.store(true, Ordering::Relaxed);
    drop(task_tx);
    for worker in workers {
        let _ = worker.join();
    }
    Ok(outcome)
}

fn scan_directory(root: &Path, task: DirectoryTask, cancelled: &AtomicBool) -> DirectoryResult {
    let mut records = Vec::new();
    let mut directories = Vec::new();
    let mut issues = Vec::new();
    let ignore = IgnoreContext::load(root, &task.path, &mut issues);

    match metadata_with_retry(&task.path) {
        Ok(metadata) => records.push(record_for_path(&task.path, task.depth, &metadata)),
        Err(error) => {
            issues.push(issue(&task.path, "metadata", &error));
            return DirectoryResult {
                task,
                records,
                directories,
                issues,
            };
        }
    }

    let read_dir = match read_dir_with_retry(&task.path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            issues.push(issue(&task.path, "read_dir", &error));
            return DirectoryResult {
                task,
                records,
                directories,
                issues,
            };
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        match entry {
            Ok(entry) => entries.push(entry.path()),
            Err(error) => issues.push(issue(&task.path, "read_entry", &error)),
        }
    }
    entries.sort_by_cached_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    });
    for path in entries {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let metadata = match metadata_with_retry(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                issues.push(issue(&path, "metadata", &error));
                continue;
            }
        };
        let kind = node_kind(&metadata);
        if ignore.ignores(&path, kind == NodeKind::Directory) {
            continue;
        }
        let depth = task.depth + 1;
        let record = record_for_path(&path, depth, &metadata);
        if kind == NodeKind::Directory {
            directories.push(DirectoryTask {
                path: path.clone(),
                path_key: record.path_key.clone(),
                depth,
                priority: 0,
            });
        }
        records.push(record);
    }
    DirectoryResult {
        task,
        records,
        directories,
        issues,
    }
}

struct IgnoreContext {
    project_root: PathBuf,
    matchers: Vec<Gitignore>,
    override_matcher: Option<Gitignore>,
}

impl IgnoreContext {
    fn load(root: &Path, directory: &Path, issues: &mut Vec<ScanIssue>) -> Self {
        let mut ancestors = Vec::new();
        let mut current = Some(directory);
        while let Some(path) = current {
            if path.starts_with(root) {
                ancestors.push(path.to_path_buf());
            }
            if path == root {
                break;
            }
            current = path.parent();
        }
        ancestors.reverse();
        let mut matchers = Vec::new();
        for ancestor in ancestors {
            let ignore_path = ancestor.join(".gitignore");
            if ignore_path.is_file() {
                if let Some(matcher) = build_ignore_matcher(&ancestor, &ignore_path, issues) {
                    matchers.push(matcher);
                }
            }
        }
        let override_path = root.join(".shitview").join("ignore");
        let override_matcher = if override_path.is_file() {
            build_ignore_matcher(root, &override_path, issues)
        } else {
            None
        };
        Self {
            project_root: root.to_path_buf(),
            matchers,
            override_matcher,
        }
    }

    fn ignores(&self, path: &Path, is_directory: bool) -> bool {
        if path != self.project_root
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == ".git" || name == ".shitview")
        {
            return true;
        }
        let mut ignored = false;
        for matcher in &self.matchers {
            match matcher.matched_path_or_any_parents(path, is_directory) {
                Match::Ignore(_) => ignored = true,
                Match::Whitelist(_) => ignored = false,
                Match::None => {}
            }
        }
        if let Some(matcher) = &self.override_matcher {
            match matcher.matched_path_or_any_parents(path, is_directory) {
                Match::Ignore(_) => ignored = true,
                Match::Whitelist(_) => ignored = false,
                Match::None => {}
            }
        }
        ignored
    }
}

fn build_ignore_matcher(
    root: &Path,
    ignore_path: &Path,
    issues: &mut Vec<ScanIssue>,
) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    if let Some(error) = builder.add(ignore_path) {
        issues.push(ScanIssue {
            path: display_path(ignore_path),
            operation: "parse_ignore".to_owned(),
            message: error.to_string(),
        });
    }
    match builder.build() {
        Ok(matcher) => Some(matcher),
        Err(error) => {
            issues.push(ScanIssue {
                path: display_path(ignore_path),
                operation: "parse_ignore".to_owned(),
                message: error.to_string(),
            });
            None
        }
    }
}

fn apply_watch_event(
    root: &Path,
    generation: &Generation,
    store: &mut IndexStore,
    event: &Event,
    events: &Sender<IndexEvent>,
) -> Result<(), String> {
    if event.paths.is_empty() {
        return Ok(());
    }
    if matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
    ) && event.paths.len() >= 2
    {
        let old_path = display_path(&event.paths[0]);
        store
            .delete_current_path(generation, &old_path)
            .map_err(|error| format!("cannot apply rename removal: {error}"))?;
        upsert_path_tree(root, generation, store, &event.paths[1], events)?;
        return Ok(());
    }
    match event.kind {
        EventKind::Remove(_) => {
            for path in &event.paths {
                store
                    .delete_current_path(generation, &display_path(path))
                    .map_err(|error| format!("cannot apply file removal: {error}"))?;
            }
        }
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any | EventKind::Other => {
            for path in &event.paths {
                if path.exists() {
                    upsert_path_tree(root, generation, store, path, events)?;
                }
            }
        }
        EventKind::Access(_) => {}
    }
    Ok(())
}

fn upsert_path_tree(
    root: &Path,
    generation: &Generation,
    store: &mut IndexStore,
    path: &Path,
    events: &Sender<IndexEvent>,
) -> Result<(), String> {
    if !path.starts_with(root) || path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        value == ".git" || value == ".shitview"
    }) {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot read changed path {}: {error}", path.display())),
    };
    let relative_depth = path
        .strip_prefix(root)
        .map(|relative| relative.components().count())
        .unwrap_or(0);
    let record = record_for_path(path, relative_depth, &metadata);
    store
        .upsert_current_records(generation, &[record])
        .map_err(|error| format!("cannot update changed path: {error}"))?;
    if node_kind(&metadata) != NodeKind::Directory {
        return Ok(());
    }
    let cancelled = AtomicBool::new(false);
    let mut queue = VecDeque::from([DirectoryTask {
        path: path.to_path_buf(),
        path_key: path_key(path),
        depth: relative_depth,
        priority: 0,
    }]);
    while let Some(task) = queue.pop_front() {
        let result = scan_directory(root, task, &cancelled);
        store
            .upsert_current_records(generation, &result.records)
            .map_err(|error| format!("cannot update changed directory: {error}"))?;
        queue.extend(result.directories);
        for issue in result.issues {
            let _ = events.send(IndexEvent::Warning(format!(
                "{}: {}",
                issue.path, issue.message
            )));
        }
    }
    Ok(())
}

fn publish_current_nodes(
    store: &IndexStore,
    generation: &Generation,
    limit: usize,
    events: &Sender<IndexEvent>,
) {
    match store.current_nodes(generation.project_id, limit) {
        Ok(nodes) => {
            let _ = events.send(IndexEvent::Nodes(nodes));
        }
        Err(error) => {
            let _ = events.send(IndexEvent::Warning(format!(
                "cannot load visible nodes: {error}"
            )));
        }
    }
}

fn emit_progress(
    events: &Sender<IndexEvent>,
    phase: IndexPhase,
    generation: &Generation,
    store: &IndexStore,
    resumed: bool,
) {
    let (indexed_nodes, issue_count, pending_directories) =
        store.counts(generation).unwrap_or_default();
    let _ = events.send(IndexEvent::Progress(IndexProgress {
        phase,
        generation: generation.number,
        indexed_nodes,
        pending_directories,
        issue_count,
        resumed,
    }));
}

fn promote_in_memory(path: &Path, high: &mut VecDeque<DirectoryTask>, normal: &mut VecDeque<DirectoryTask>) {
    let mut retained = VecDeque::new();
    while let Some(mut task) = normal.pop_front() {
        if path.starts_with(&task.path) || task.path.starts_with(path) {
            task.priority = 100;
            high.push_back(task);
        } else {
            retained.push_back(task);
        }
    }
    *normal = retained;
}

fn pending_from_task(task: &DirectoryTask) -> PendingDirectory {
    PendingDirectory {
        path_key: task.path_key.clone(),
        display_path: display_path(&task.path),
        depth: task.depth,
        priority: task.priority,
    }
}

fn record_for_path(path: &Path, depth: usize, metadata: &Metadata) -> IndexRecord {
    IndexRecord {
        path_key: path_key(path),
        parent_path_key: path.parent().map(path_key),
        display_path: display_path(path),
        display_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| display_path(path)),
        kind: node_kind(metadata),
        depth,
        size_bytes: if metadata.is_file() { metadata.len() } else { 0 },
        modified_ns: metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos()),
        stable_id: stable_file_id(path, metadata),
    }
}

fn node_kind(metadata: &Metadata) -> NodeKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        NodeKind::Symlink
    } else if file_type.is_dir() {
        NodeKind::Directory
    } else if file_type.is_file() {
        NodeKind::File
    } else {
        NodeKind::Other
    }
}

#[cfg(windows)]
fn stable_file_id(path: &Path, _metadata: &Metadata) -> Option<Vec<u8>> {
    use std::fs::OpenOptions;
    use std::mem::zeroed;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let result = unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), &mut information)
    };
    if result == 0 {
        return None;
    }
    let volume = information.dwVolumeSerialNumber;
    let index = (u64::from(information.nFileIndexHigh) << 32)
        | u64::from(information.nFileIndexLow);
    let mut value = Vec::with_capacity(12);
    value.extend_from_slice(&volume.to_le_bytes());
    value.extend_from_slice(&index.to_le_bytes());
    Some(value)
}

#[cfg(unix)]
fn stable_file_id(_path: &Path, metadata: &Metadata) -> Option<Vec<u8>> {
    use std::os::unix::fs::MetadataExt;
    let mut value = Vec::with_capacity(16);
    value.extend_from_slice(&metadata.dev().to_le_bytes());
    value.extend_from_slice(&metadata.ino().to_le_bytes());
    Some(value)
}

#[cfg(not(any(windows, unix)))]
fn stable_file_id(_path: &Path, _metadata: &Metadata) -> Option<Vec<u8>> {
    None
}

fn metadata_with_retry(path: &Path) -> io::Result<Metadata> {
    retry_io(|| fs::symlink_metadata(path))
}

fn read_dir_with_retry(path: &Path) -> io::Result<fs::ReadDir> {
    retry_io(|| fs::read_dir(path))
}

fn retry_io<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last_error = None;
    for attempt in 0..2 {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last_error = Some(error);
                if attempt == 0 {
                    thread::sleep(Duration::from_millis(12));
                }
            }
        }
    }
    Err(last_error.expect("retry loop always records an error"))
}

fn issue(path: &Path, operation: &str, error: &io::Error) -> ScanIssue {
    ScanIssue {
        path: display_path(path),
        operation: operation.to_owned(),
        message: error.to_string(),
    }
}

fn project_key(path: &Path) -> String {
    let display = display_path(path);
    if cfg!(windows) {
        display.to_lowercase()
    } else {
        display
    }
}

fn display_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if let Some(unc_path) = normalized.strip_prefix("//?/UNC/") {
        return format!("//{unc_path}");
    }
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_owned()
}

#[cfg(windows)]
fn path_key(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn path_key(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(any(windows, unix)))]
fn path_key(path: &Path) -> Vec<u8> {
    display_path(path).into_bytes()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn event_kind_name(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(ModifyKind::Name(_)) => "rename",
        EventKind::Modify(_) => "modify",
        EventKind::Remove(_) => "remove",
        EventKind::Access(_) => "access",
        EventKind::Any => "any",
        EventKind::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::{default_database_path, fnv1a64, path_key, IndexEvent, IndexHandle, IndexOptions, IndexPhase};
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("shitview-indexer-{stamp}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn database_path_is_stable_for_a_project() {
        let path = PathBuf::from("H:/project");
        assert_eq!(default_database_path(&path), default_database_path(&path));
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
    }

    #[test]
    fn indexes_a_real_tree_and_respects_gitignore() {
        let temp = TempDir::new();
        fs::create_dir(temp.0.join("src")).unwrap();
        fs::create_dir(temp.0.join("ignored")).unwrap();
        File::create(temp.0.join("src/main.rs")).unwrap();
        File::create(temp.0.join("ignored/large.bin")).unwrap();
        fs::write(temp.0.join(".gitignore"), "ignored/\n").unwrap();
        let database = temp.0.join("index.sqlite3");
        let handle = IndexHandle::start(
            &temp.0,
            database,
            IndexOptions {
                worker_count: 2,
                watch: false,
                ..IndexOptions::default()
            },
        )
        .unwrap();
        let receiver = handle.events();
        let mut nodes = Vec::new();
        let mut complete = false;
        while let Ok(event) = receiver.recv_timeout(Duration::from_secs(10)) {
            match event {
                IndexEvent::Nodes(value) => nodes = value,
                IndexEvent::Progress(progress)
                    if matches!(progress.phase, IndexPhase::Complete | IndexPhase::CompleteWithWarnings) =>
                {
                    complete = true;
                    break;
                }
                IndexEvent::Failed(error) => panic!("index failed: {error}"),
                _ => {}
            }
        }
        assert!(complete);
        assert!(nodes.iter().any(|node| node.display_name == "main.rs"));
        assert!(!nodes.iter().any(|node| node.display_name == "large.bin"));
        assert!(!path_key(&temp.0).is_empty());
    }

    #[test]
    fn paused_scan_resumes_the_same_generation() {
        let temp = TempDir::new();
        let project = temp.0.join("project");
        fs::create_dir(&project).unwrap();
        for index in 0..200 {
            let directory = project.join(format!("dir-{index:03}"));
            fs::create_dir(&directory).unwrap();
            File::create(directory.join("file.txt")).unwrap();
        }
        let database = temp.0.join("resume.sqlite3");
        let handle = IndexHandle::start(
            &project,
            &database,
            IndexOptions {
                worker_count: 1,
                watch: false,
                ..IndexOptions::default()
            },
        )
        .unwrap();
        let receiver = handle.events();
        handle.pause().unwrap();
        let paused = receiver
            .iter()
            .find_map(|event| match event {
                IndexEvent::Progress(progress) if progress.phase == IndexPhase::Paused => {
                    Some(progress)
                }
                _ => None,
            })
            .unwrap();
        let generation = paused.generation;
        drop(handle);

        let handle = IndexHandle::start(
            &project,
            &database,
            IndexOptions {
                worker_count: 2,
                watch: false,
                ..IndexOptions::default()
            },
        )
        .unwrap();
        let receiver = handle.events();
        let completed = receiver
            .iter()
            .find_map(|event| match event {
                IndexEvent::Progress(progress)
                    if matches!(progress.phase, IndexPhase::Complete | IndexPhase::CompleteWithWarnings) =>
                {
                    Some(progress)
                }
                IndexEvent::Failed(error) => panic!("resumed index failed: {error}"),
                _ => None,
            })
            .unwrap();
        assert_eq!(completed.generation, generation);
        assert!(completed.resumed);
        assert!(completed.indexed_nodes >= 401);
    }

    #[test]
    fn watcher_adds_a_new_file_incrementally() {
        let temp = TempDir::new();
        let project = temp.0.join("watched-project");
        fs::create_dir(&project).unwrap();
        File::create(project.join("initial.txt")).unwrap();
        let handle = IndexHandle::start(
            &project,
            temp.0.join("watch.sqlite3"),
            IndexOptions {
                worker_count: 2,
                watch: true,
                ..IndexOptions::default()
            },
        )
        .unwrap();
        let receiver = handle.events();
        let watching = receiver
            .iter()
            .find_map(|event| match event {
                IndexEvent::Progress(progress) if progress.phase == IndexPhase::Watching => {
                    Some(progress)
                }
                IndexEvent::Failed(error) => panic!("watched index failed: {error}"),
                _ => None,
            })
            .unwrap();
        assert!(watching.indexed_nodes >= 2);
        File::create(project.join("created-after-scan.txt")).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut observed = false;
        while std::time::Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(IndexEvent::Nodes(nodes)) => {
                    if nodes
                        .iter()
                        .any(|node| node.display_name == "created-after-scan.txt")
                    {
                        observed = true;
                        break;
                    }
                }
                Ok(IndexEvent::Failed(error)) => panic!("watch update failed: {error}"),
                Ok(_) | Err(_) => {}
            }
        }
        assert!(observed, "native watcher did not publish the new file");
    }
}
