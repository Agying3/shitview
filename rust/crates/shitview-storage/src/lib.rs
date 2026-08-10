use rusqlite::{params, Connection, Result};
use shitview_core::{IndexRecord, NodeKind, ScanIssue};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationStatus {
    Running,
    Paused,
    Complete,
    CompleteWithWarnings,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    pub project_id: i64,
    pub number: i64,
    pub resumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDirectory {
    pub path_key: Vec<u8>,
    pub display_path: String,
    pub depth: usize,
    pub priority: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNode {
    pub stable_id: Option<Vec<u8>>,
    pub display_path: String,
    pub display_name: String,
    pub kind: NodeKind,
    pub depth: usize,
    pub size_bytes: u64,
}

pub struct IndexStore {
    connection: Connection,
}

#[derive(Debug, Clone, Copy)]
pub struct StorageBenchmark {
    pub rows: usize,
    pub insert_elapsed: Duration,
    pub count_elapsed: Duration,
    pub page_count: i64,
    pub page_size: i64,
}

pub fn configure_connection(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;",
    )
}

pub fn initialize_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
             id INTEGER PRIMARY KEY,
             project_key TEXT NOT NULL UNIQUE,
             root_display TEXT NOT NULL,
             scan_generation INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS nodes (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             parent_id INTEGER REFERENCES nodes(id) ON DELETE CASCADE,
             path_key BLOB NOT NULL,
             display_name TEXT NOT NULL,
             kind INTEGER NOT NULL,
             depth INTEGER NOT NULL,
             size_bytes INTEGER NOT NULL DEFAULT 0,
             modified_ns INTEGER,
             content_hash BLOB,
             scan_generation INTEGER NOT NULL,
             UNIQUE(project_id, path_key)
         );
         CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(project_id, parent_id);
         CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(project_id, display_name);
         CREATE INDEX IF NOT EXISTS idx_nodes_generation ON nodes(project_id, scan_generation);",
    )
}

pub fn initialize_index_schema(connection: &Connection) -> Result<()> {
    initialize_schema(connection)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS scan_generations (
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             generation INTEGER NOT NULL,
             status TEXT NOT NULL,
             started_ms INTEGER NOT NULL,
             updated_ms INTEGER NOT NULL,
             completed_ms INTEGER,
             discovered_count INTEGER NOT NULL DEFAULT 0,
             indexed_count INTEGER NOT NULL DEFAULT 0,
             issue_count INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY(project_id, generation)
         );
         CREATE TABLE IF NOT EXISTS index_nodes (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             generation INTEGER NOT NULL,
             stable_id BLOB,
             parent_path_key BLOB,
             path_key BLOB NOT NULL,
             display_path TEXT NOT NULL,
             display_name TEXT NOT NULL,
             kind INTEGER NOT NULL,
             depth INTEGER NOT NULL,
             size_bytes INTEGER NOT NULL DEFAULT 0,
             modified_ns TEXT,
             UNIQUE(project_id, generation, path_key)
         );
         CREATE INDEX IF NOT EXISTS idx_index_nodes_parent
             ON index_nodes(project_id, generation, parent_path_key);
         CREATE INDEX IF NOT EXISTS idx_index_nodes_stable
             ON index_nodes(project_id, generation, stable_id);
         CREATE INDEX IF NOT EXISTS idx_index_nodes_display_path
             ON index_nodes(project_id, generation, display_path);
         CREATE TABLE IF NOT EXISTS scan_queue (
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             generation INTEGER NOT NULL,
             path_key BLOB NOT NULL,
             display_path TEXT NOT NULL,
             depth INTEGER NOT NULL,
             priority INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY(project_id, generation, path_key)
         );
         CREATE INDEX IF NOT EXISTS idx_scan_queue_priority
             ON scan_queue(project_id, generation, priority DESC, depth ASC);
         CREATE TABLE IF NOT EXISTS scan_issues (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             generation INTEGER NOT NULL,
             path TEXT NOT NULL,
             operation TEXT NOT NULL,
             message TEXT NOT NULL,
             occurrences INTEGER NOT NULL DEFAULT 1,
             UNIQUE(project_id, generation, path, operation, message)
         );
         CREATE TABLE IF NOT EXISTS watch_events (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             generation INTEGER NOT NULL,
             event_kind TEXT NOT NULL,
             path TEXT NOT NULL,
             secondary_path TEXT,
             created_ms INTEGER NOT NULL,
             applied INTEGER NOT NULL DEFAULT 0
         );",
    )
}

impl IndexStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
        let connection = Connection::open(path)?;
        configure_connection(&connection)?;
        initialize_index_schema(&connection)?;
        Ok(Self { connection })
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        configure_connection(&connection)?;
        initialize_index_schema(&connection)?;
        Ok(Self { connection })
    }

    pub fn begin_or_resume(
        &mut self,
        project_key: &str,
        root_display: &str,
        now_ms: i64,
    ) -> Result<Generation> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO projects(project_key, root_display)
             VALUES(?1, ?2)
             ON CONFLICT(project_key) DO UPDATE SET root_display=excluded.root_display",
            params![project_key, root_display],
        )?;
        let project_id: i64 = transaction.query_row(
            "SELECT id FROM projects WHERE project_key=?1",
            [project_key],
            |row| row.get(0),
        )?;
        let resumable = transaction
            .query_row(
                "SELECT generation FROM scan_generations
                 WHERE project_id=?1 AND status IN ('running', 'paused')
                 ORDER BY generation DESC LIMIT 1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .ok();
        let (number, resumed) = if let Some(generation) = resumable {
            transaction.execute(
                "UPDATE scan_generations SET status='running', updated_ms=?3
                 WHERE project_id=?1 AND generation=?2",
                params![project_id, generation, now_ms],
            )?;
            (generation, true)
        } else {
            let generation: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(generation), 0) + 1 FROM scan_generations WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO scan_generations(
                     project_id, generation, status, started_ms, updated_ms
                 ) VALUES(?1, ?2, 'running', ?3, ?3)",
                params![project_id, generation, now_ms],
            )?;
            (generation, false)
        };
        transaction.commit()?;
        Ok(Generation {
            project_id,
            number,
            resumed,
        })
    }

    pub fn enqueue_directories(
        &mut self,
        generation: &Generation,
        directories: &[PendingDirectory],
    ) -> Result<()> {
        if directories.is_empty() {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO scan_queue(
                     project_id, generation, path_key, display_path, depth, priority
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project_id, generation, path_key) DO UPDATE SET
                     priority=MAX(priority, excluded.priority)",
            )?;
            for directory in directories {
                statement.execute(params![
                    generation.project_id,
                    generation.number,
                    directory.path_key,
                    directory.display_path,
                    directory.depth as i64,
                    directory.priority,
                ])?;
            }
        }
        transaction.commit()
    }

    pub fn pending_directories(&self, generation: &Generation) -> Result<Vec<PendingDirectory>> {
        let mut statement = self.connection.prepare(
            "SELECT path_key, display_path, depth, priority FROM scan_queue
             WHERE project_id=?1 AND generation=?2
             ORDER BY priority DESC, depth ASC, display_path ASC",
        )?;
        let rows = statement.query_map(
            params![generation.project_id, generation.number],
            |row| {
                Ok(PendingDirectory {
                    path_key: row.get(0)?,
                    display_path: row.get(1)?,
                    depth: row.get::<_, i64>(2)? as usize,
                    priority: row.get(3)?,
                })
            },
        )?;
        rows.collect()
    }

    pub fn commit_directory(
        &mut self,
        generation: &Generation,
        completed_path_key: &[u8],
        records: &[IndexRecord],
        discovered_directories: &[PendingDirectory],
        issues: &[ScanIssue],
        now_ms: i64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO index_nodes(
                     project_id, generation, stable_id, parent_path_key, path_key,
                     display_path, display_name, kind, depth, size_bytes, modified_ns
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(project_id, generation, path_key) DO UPDATE SET
                     stable_id=excluded.stable_id,
                     parent_path_key=excluded.parent_path_key,
                     display_path=excluded.display_path,
                     display_name=excluded.display_name,
                     kind=excluded.kind,
                     depth=excluded.depth,
                     size_bytes=excluded.size_bytes,
                     modified_ns=excluded.modified_ns",
            )?;
            for record in records {
                statement.execute(params![
                    generation.project_id,
                    generation.number,
                    record.stable_id,
                    record.parent_path_key,
                    record.path_key,
                    record.display_path,
                    record.display_name,
                    kind_to_i64(record.kind),
                    record.depth as i64,
                    saturating_i64(record.size_bytes),
                    record.modified_ns.map(|value| value.to_string()),
                ])?;
            }
        }
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO scan_queue(
                     project_id, generation, path_key, display_path, depth, priority
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project_id, generation, path_key) DO UPDATE SET
                     priority=MAX(priority, excluded.priority)",
            )?;
            for directory in discovered_directories {
                statement.execute(params![
                    generation.project_id,
                    generation.number,
                    directory.path_key,
                    directory.display_path,
                    directory.depth as i64,
                    directory.priority,
                ])?;
            }
        }
        transaction.execute(
            "DELETE FROM scan_queue WHERE project_id=?1 AND generation=?2 AND path_key=?3",
            params![generation.project_id, generation.number, completed_path_key],
        )?;
        for issue in issues {
            transaction.execute(
                "INSERT INTO scan_issues(project_id, generation, path, operation, message)
                 VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project_id, generation, path, operation, message)
                 DO UPDATE SET occurrences=occurrences + 1",
                params![
                    generation.project_id,
                    generation.number,
                    issue.path,
                    issue.operation,
                    issue.message,
                ],
            )?;
        }
        transaction.execute(
            "UPDATE scan_generations SET
                 updated_ms=?3,
                 discovered_count=discovered_count + ?4,
                 indexed_count=(SELECT COUNT(*) FROM index_nodes WHERE project_id=?1 AND generation=?2),
                 issue_count=(SELECT COALESCE(SUM(occurrences), 0) FROM scan_issues WHERE project_id=?1 AND generation=?2)
             WHERE project_id=?1 AND generation=?2",
            params![
                generation.project_id,
                generation.number,
                now_ms,
                discovered_directories.len() as i64,
            ],
        )?;
        transaction.commit()
    }

    pub fn promote_path(
        &self,
        generation: &Generation,
        display_path: &str,
    ) -> Result<usize> {
        self.connection.execute(
            "UPDATE scan_queue SET priority=100
             WHERE project_id=?1 AND generation=?2
               AND (?3 = display_path OR ?3 LIKE display_path || '/%')",
            params![generation.project_id, generation.number, display_path],
        )
    }

    pub fn pause_generation(&self, generation: &Generation, now_ms: i64) -> Result<()> {
        set_generation_status(
            &self.connection,
            generation,
            GenerationStatus::Paused,
            now_ms,
        )
    }

    pub fn fail_generation(&self, generation: &Generation, now_ms: i64) -> Result<()> {
        set_generation_status(
            &self.connection,
            generation,
            GenerationStatus::Failed,
            now_ms,
        )
    }

    pub fn cancel_generation(&mut self, generation: &Generation, now_ms: i64) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE scan_generations SET status='cancelled', updated_ms=?3, completed_ms=?3
             WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number, now_ms],
        )?;
        transaction.execute(
            "DELETE FROM index_nodes WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
        )?;
        transaction.execute(
            "DELETE FROM scan_queue WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
        )?;
        transaction.execute(
            "DELETE FROM scan_issues WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
        )?;
        transaction.commit()
    }

    pub fn complete_generation(&mut self, generation: &Generation, now_ms: i64) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let pending: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM scan_queue WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
            |row| row.get(0),
        )?;
        if pending != 0 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let issue_count: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(occurrences), 0) FROM scan_issues
             WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
            |row| row.get(0),
        )?;
        let status = if issue_count == 0 {
            GenerationStatus::Complete
        } else {
            GenerationStatus::CompleteWithWarnings
        };
        transaction.execute(
            "UPDATE scan_generations SET status=?3, updated_ms=?4, completed_ms=?4
             WHERE project_id=?1 AND generation=?2",
            params![
                generation.project_id,
                generation.number,
                status.as_str(),
                now_ms,
            ],
        )?;
        transaction.execute(
            "UPDATE projects SET scan_generation=?2 WHERE id=?1",
            params![generation.project_id, generation.number],
        )?;
        transaction.commit()
    }

    pub fn counts(&self, generation: &Generation) -> Result<(usize, usize, usize)> {
        self.connection.query_row(
            "SELECT indexed_count, issue_count,
                    (SELECT COUNT(*) FROM scan_queue WHERE project_id=?1 AND generation=?2)
             FROM scan_generations WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as usize,
                ))
            },
        )
    }

    pub fn current_nodes(&self, project_id: i64, limit: usize) -> Result<Vec<StoredNode>> {
        let generation: i64 = self.connection.query_row(
            "SELECT scan_generation FROM projects WHERE id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT stable_id, display_path, display_name, kind, depth, size_bytes
             FROM index_nodes WHERE project_id=?1 AND generation=?2
             ORDER BY depth, display_path LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![project_id, generation, limit as i64],
            |row| {
                Ok(StoredNode {
                    stable_id: row.get(0)?,
                    display_path: row.get(1)?,
                    display_name: row.get(2)?,
                    kind: i64_to_kind(row.get(3)?),
                    depth: row.get::<_, i64>(4)? as usize,
                    size_bytes: row.get::<_, i64>(5)? as u64,
                })
            },
        )?;
        rows.collect()
    }

    pub fn current_generation_for_project_key(&self, project_key: &str) -> Result<Option<Generation>> {
        let mut statement = self.connection.prepare(
            "SELECT id, scan_generation FROM projects WHERE project_key=?1",
        )?;
        let mut rows = statement.query([project_key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let project_id: i64 = row.get(0)?;
        let number: i64 = row.get(1)?;
        if number == 0 {
            Ok(None)
        } else {
            Ok(Some(Generation {
                project_id,
                number,
                resumed: false,
            }))
        }
    }

    pub fn upsert_current_records(
        &mut self,
        generation: &Generation,
        records: &[IndexRecord],
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO index_nodes(
                     project_id, generation, stable_id, parent_path_key, path_key,
                     display_path, display_name, kind, depth, size_bytes, modified_ns
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(project_id, generation, path_key) DO UPDATE SET
                     stable_id=excluded.stable_id,
                     parent_path_key=excluded.parent_path_key,
                     display_path=excluded.display_path,
                     display_name=excluded.display_name,
                     kind=excluded.kind,
                     depth=excluded.depth,
                     size_bytes=excluded.size_bytes,
                     modified_ns=excluded.modified_ns",
            )?;
            for record in records {
                statement.execute(params![
                    generation.project_id,
                    generation.number,
                    record.stable_id,
                    record.parent_path_key,
                    record.path_key,
                    record.display_path,
                    record.display_name,
                    kind_to_i64(record.kind),
                    record.depth as i64,
                    saturating_i64(record.size_bytes),
                    record.modified_ns.map(|value| value.to_string()),
                ])?;
            }
        }
        transaction.execute(
            "UPDATE scan_generations SET indexed_count=(
                 SELECT COUNT(*) FROM index_nodes WHERE project_id=?1 AND generation=?2
             ) WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
        )?;
        transaction.commit()
    }

    pub fn delete_current_path(
        &mut self,
        generation: &Generation,
        display_path: &str,
    ) -> Result<usize> {
        let escaped = escape_like(display_path);
        let pattern = format!("{escaped}/%");
        let transaction = self.connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM index_nodes
             WHERE project_id=?1 AND generation=?2
               AND (display_path=?3 OR display_path LIKE ?4 ESCAPE '\\')",
            params![
                generation.project_id,
                generation.number,
                display_path,
                pattern,
            ],
        )?;
        transaction.execute(
            "UPDATE scan_generations SET indexed_count=(
                 SELECT COUNT(*) FROM index_nodes WHERE project_id=?1 AND generation=?2
             ) WHERE project_id=?1 AND generation=?2",
            params![generation.project_id, generation.number],
        )?;
        transaction.commit()?;
        Ok(deleted)
    }

    pub fn rename_current_path(
        &mut self,
        generation: &Generation,
        old_display_path: &str,
        new_display_path: &str,
        new_path_key: &[u8],
        new_parent_path_key: Option<&[u8]>,
    ) -> Result<usize> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE index_nodes SET
                 path_key=?4,
                 parent_path_key=?5,
                 display_path=?6,
                 display_name=?7
             WHERE project_id=?1 AND generation=?2 AND display_path=?3",
            params![
                generation.project_id,
                generation.number,
                old_display_path,
                new_path_key,
                new_parent_path_key,
                new_display_path,
                Path::new(new_display_path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| new_display_path.to_owned()),
            ],
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn record_watch_event(
        &self,
        generation: &Generation,
        event_kind: &str,
        path: &str,
        secondary_path: Option<&str>,
        now_ms: i64,
    ) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO watch_events(
                 project_id, generation, event_kind, path, secondary_path, created_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                generation.project_id,
                generation.number,
                event_kind,
                path,
                secondary_path,
                now_ms,
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn mark_watch_event_applied(&self, event_id: i64) -> Result<()> {
        self.connection.execute(
            "UPDATE watch_events SET applied=1 WHERE id=?1",
            [event_id],
        )?;
        Ok(())
    }
}

impl GenerationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::CompleteWithWarnings => "complete_with_warnings",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

fn set_generation_status(
    connection: &Connection,
    generation: &Generation,
    status: GenerationStatus,
    now_ms: i64,
) -> Result<()> {
    connection.execute(
        "UPDATE scan_generations SET status=?3, updated_ms=?4
         WHERE project_id=?1 AND generation=?2",
        params![
            generation.project_id,
            generation.number,
            status.as_str(),
            now_ms,
        ],
    )?;
    Ok(())
}

fn kind_to_i64(kind: NodeKind) -> i64 {
    match kind {
        NodeKind::Directory => 1,
        NodeKind::File => 2,
        NodeKind::Symlink => 3,
        NodeKind::Other => 4,
    }
}

fn i64_to_kind(value: i64) -> NodeKind {
    match value {
        1 => NodeKind::Directory,
        2 => NodeKind::File,
        3 => NodeKind::Symlink,
        _ => NodeKind::Other,
    }
}

fn saturating_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn run_synthetic_benchmark(connection: &mut Connection, rows: usize) -> Result<StorageBenchmark> {
    configure_connection(connection)?;
    initialize_schema(connection)?;
    connection.execute(
        "INSERT OR IGNORE INTO projects(id, project_key, root_display) VALUES(1, ?1, ?2)",
        params!["phase0-synthetic", "H:/synthetic"],
    )?;

    let insert_started = Instant::now();
    let transaction = connection.transaction()?;
    {
        let mut statement = transaction.prepare_cached(
            "INSERT OR REPLACE INTO nodes(
                 id, project_id, parent_id, path_key, display_name, kind, depth,
                 size_bytes, modified_ns, content_hash, scan_generation
             ) VALUES(?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 1)",
        )?;
        for index in 0..rows {
            let id = index as i64 + 1;
            let parent_id = if index > 0 {
                Some(((index - 1) / 64 + 1) as i64)
            } else {
                None
            };
            let module = index / 10_000;
            let path = format!("H:/synthetic/module_{module:04}/node_{index:09}");
            let name = format!("node_{index:09}");
            let kind = if index % 64 == 0 { 1 } else { 2 };
            let depth = if index == 0 { 0 } else { 2 };
            statement.execute(params![
                id,
                parent_id,
                path.as_bytes(),
                name,
                kind,
                depth,
                (index % 1_048_576) as i64,
                1_786_280_000_000_000_000_i64 + index as i64,
            ])?;
        }
    }
    transaction.commit()?;
    let insert_elapsed = insert_started.elapsed();

    let count_started = Instant::now();
    let stored_rows: usize = connection.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
    let count_elapsed = count_started.elapsed();
    let page_count = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let page_size = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;

    Ok(StorageBenchmark {
        rows: stored_rows,
        insert_elapsed,
        count_elapsed,
        page_count,
        page_size,
    })
}

#[cfg(test)]
mod tests {
    use super::{initialize_schema, run_synthetic_benchmark, IndexStore, PendingDirectory};
    use shitview_core::{IndexRecord, NodeKind};
    use rusqlite::Connection;

    #[test]
    fn creates_the_phase_zero_schema() {
        let connection = Connection::open_in_memory().unwrap();
        initialize_schema(&connection).unwrap();
        let table_count: usize = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('projects', 'nodes')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 2);
    }

    #[test]
    fn inserts_and_counts_synthetic_nodes() {
        let mut connection = Connection::open_in_memory().unwrap();
        let result = run_synthetic_benchmark(&mut connection, 10_000).unwrap();
        assert_eq!(result.rows, 10_000);
        assert!(result.page_count > 0);
        assert!(result.page_size > 0);
    }

    #[test]
    fn enables_wal_for_file_databases() {
        let path = std::env::temp_dir().join(format!(
            "shitview-storage-wal-{}.db",
            std::process::id()
        ));
        let journal_mode = {
            let connection = Connection::open(&path).unwrap();
            super::configure_connection(&connection).unwrap();
            connection
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap()
        };
        let _ = std::fs::remove_file(path);
        assert_eq!(journal_mode, "wal");
    }

    #[test]
    fn generation_switch_keeps_queue_and_nodes_atomic() {
        let mut store = IndexStore::open_in_memory().unwrap();
        let generation = store
            .begin_or_resume("test-project", "H:/project", 10)
            .unwrap();
        let root = PendingDirectory {
            path_key: b"root".to_vec(),
            display_path: "H:/project".to_owned(),
            depth: 0,
            priority: 0,
        };
        store.enqueue_directories(&generation, &[root]).unwrap();
        let record = IndexRecord {
            path_key: b"root".to_vec(),
            parent_path_key: None,
            display_path: "H:/project".to_owned(),
            display_name: "project".to_owned(),
            kind: NodeKind::Directory,
            depth: 0,
            size_bytes: 0,
            modified_ns: None,
            stable_id: Some(b"stable-root".to_vec()),
        };
        store
            .commit_directory(&generation, b"root", &[record], &[], &[], 20)
            .unwrap();
        assert_eq!(store.counts(&generation).unwrap(), (1, 0, 0));
        store.complete_generation(&generation, 30).unwrap();
        let current = store
            .current_generation_for_project_key("test-project")
            .unwrap()
            .unwrap();
        assert_eq!(current.number, generation.number);
        assert_eq!(store.current_nodes(current.project_id, 10).unwrap().len(), 1);
    }

    #[test]
    fn cancelled_generation_is_not_visible() {
        let mut store = IndexStore::open_in_memory().unwrap();
        let generation = store
            .begin_or_resume("cancelled-project", "H:/cancelled", 10)
            .unwrap();
        store.cancel_generation(&generation, 20).unwrap();
        assert!(store
            .current_generation_for_project_key("cancelled-project")
            .unwrap()
            .is_none());
    }
}
