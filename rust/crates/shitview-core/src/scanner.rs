use crate::{NodeKind, ScanIssue, ScanStats, Snapshot, SnapshotNode};
use std::collections::HashSet;
use std::fs::{self, DirEntry, Metadata};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct IgnoreRules {
    pub names: HashSet<String>,
    pub suffixes: Vec<String>,
}

impl Default for IgnoreRules {
    fn default() -> Self {
        Self {
            names: [
                ".git",
                "__pycache__",
                ".venv",
                "node_modules",
                "target",
                ".shitview",
                ".pytest_cache",
                ".mypy_cache",
                ".ruff_cache",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            suffixes: vec![".egg-info".to_owned()],
        }
    }
}

impl IgnoreRules {
    fn ignores(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            return false;
        };
        self.names.contains(name)
            || self
                .suffixes
                .iter()
                .any(|suffix| name.ends_with(suffix))
    }
}

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub max_nodes: usize,
    pub max_children_per_directory: usize,
    pub rules: IgnoreRules,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            max_nodes: 1_600,
            max_children_per_directory: 180,
            rules: IgnoreRules::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scanner {
    config: ScanConfig,
}

impl Scanner {
    pub fn new(config: ScanConfig) -> Self {
        Self { config }
    }

    pub fn scan(&self, root: impl AsRef<Path>) -> Snapshot {
        let root = root
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| root.as_ref().to_path_buf());
        let root_display = normalize_path(&root);
        let mut state = ScanState {
            config: &self.config,
            nodes: Vec::new(),
            issues: Vec::new(),
            stats: ScanStats::default(),
        };

        if self.config.max_nodes == 0 {
            state.stats.is_truncated = true;
            state.stats.omitted_nodes = 1;
        } else {
            state.visit(&root, 0);
        }

        Snapshot {
            root: root_display,
            generated_at_unix_ms: now_unix_ms(),
            nodes: state.nodes,
            issues: state.issues,
            stats: state.stats,
        }
    }
}

struct ScanState<'a> {
    config: &'a ScanConfig,
    nodes: Vec<SnapshotNode>,
    issues: Vec<ScanIssue>,
    stats: ScanStats,
}

impl ScanState<'_> {
    fn visit(&mut self, path: &Path, depth: usize) -> Option<usize> {
        if self.nodes.len() >= self.config.max_nodes {
            self.stats.omitted_nodes += 1;
            self.stats.is_truncated = true;
            return None;
        }

        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.record_issue(path, "metadata", &error.to_string());
                return None;
            }
        };
        let kind = node_kind(&metadata);
        let node_index = self.nodes.len();
        let placeholder = SnapshotNode {
            path: normalize_path(path),
            name: file_name(path),
            kind,
            depth,
            size: file_size(&metadata),
            modified_unix_ms: modified_unix_ms(&metadata),
            children: Vec::new(),
            child_count: 0,
            truncated: false,
        };
        self.nodes.push(placeholder);
        self.count_kind(kind);

        if kind == NodeKind::Directory {
            let children = self.list_children(path);
            let total_children = children.len();
            let mut child_indices = Vec::new();
            let omitted = total_children.saturating_sub(self.config.max_children_per_directory);
            for entry in children
                .into_iter()
                .take(self.config.max_children_per_directory)
            {
                if let Some(child_index) = self.visit(&entry.path(), depth + 1) {
                    child_indices.push(child_index);
                }
            }
            if omitted > 0 {
                self.stats.omitted_nodes += omitted;
                self.stats.is_truncated = true;
            }
            let total_size: u64 = child_indices
                .iter()
                .map(|index| self.nodes[*index].size)
                .sum();
            let node = &mut self.nodes[node_index];
            node.children = child_indices;
            node.child_count = total_children;
            node.truncated = omitted > 0;
            node.size = total_size;
        }

        Some(node_index)
    }

    fn list_children(&mut self, path: &Path) -> Vec<DirEntry> {
        let read_dir = match fs::read_dir(path) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                self.record_issue(path, "read_dir", &error.to_string());
                return Vec::new();
            }
        };
        let mut entries = Vec::new();
        for entry in read_dir {
            match entry {
                Ok(entry) if !self.config.rules.ignores(&entry.path()) => entries.push(entry),
                Ok(_) => {}
                Err(error) => self.record_issue(path, "read_entry", &error.to_string()),
            }
        }
        entries.sort_by_cached_key(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
        });
        entries
    }

    fn count_kind(&mut self, kind: NodeKind) {
        match kind {
            NodeKind::Directory => self.stats.directory_count += 1,
            NodeKind::File => self.stats.file_count += 1,
            NodeKind::Symlink => self.stats.symlink_count += 1,
            NodeKind::Other => self.stats.other_count += 1,
        }
        self.stats.scanned_nodes = self.nodes.len();
    }

    fn record_issue(&mut self, path: &Path, operation: &str, message: &str) {
        self.issues.push(ScanIssue {
            path: normalize_path(path),
            operation: operation.to_owned(),
            message: message.to_owned(),
        });
        self.stats.issue_count = self.issues.len();
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

fn file_size(metadata: &Metadata) -> u64 {
    if metadata.is_file() {
        metadata.len()
    } else {
        0
    }
}

fn modified_unix_ms(metadata: &Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn file_name(path: &Path) -> String {
    match path.file_name().and_then(|value| value.to_str()) {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => path.to_string_lossy().into_owned(),
    }
}

fn normalize_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if let Some(unc_path) = normalized.strip_prefix("//?/UNC/") {
        return format!("//{unc_path}");
    }
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_owned()
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{IgnoreRules, ScanConfig, Scanner};
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("shitview-core-{stamp}"));
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
    fn applies_a_global_node_limit() {
        let temp = TempDir::new();
        fs::create_dir(temp.0.join("a")).unwrap();
        fs::create_dir(temp.0.join("b")).unwrap();
        File::create(temp.0.join("a/file.txt")).unwrap();
        File::create(temp.0.join("b/file.txt")).unwrap();

        let scanner = Scanner::new(ScanConfig {
            max_nodes: 3,
            ..ScanConfig::default()
        });
        let snapshot = scanner.scan(&temp.0);
        assert!(snapshot.nodes.len() <= 3);
        assert!(snapshot.stats.is_truncated);
        assert!(snapshot.stats.omitted_nodes > 0);
    }

    #[test]
    fn ignores_generated_directories_and_suffixes() {
        let temp = TempDir::new();
        fs::create_dir(temp.0.join(".git")).unwrap();
        fs::create_dir(temp.0.join("package.egg-info")).unwrap();
        File::create(temp.0.join("visible.txt")).unwrap();

        let snapshot = Scanner::new(ScanConfig {
            rules: IgnoreRules::default(),
            ..ScanConfig::default()
        })
        .scan(&temp.0);

        assert_eq!(snapshot.nodes.len(), 2);
        assert!(snapshot
            .nodes
            .iter()
            .all(|node| !node.path.contains(".git") && !node.path.contains("egg-info")));
    }

    #[test]
    fn normalizes_windows_extended_paths() {
        let path = std::path::Path::new(r"\\?\H:\shitview");
        assert_eq!(super::normalize_path(path), "H:/shitview");
    }
}
