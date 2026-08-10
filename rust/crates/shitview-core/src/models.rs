#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotNode {
    pub path: String,
    pub name: String,
    pub kind: NodeKind,
    pub depth: usize,
    pub size: u64,
    pub modified_unix_ms: Option<u128>,
    pub children: Vec<usize>,
    pub child_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    pub path: String,
    pub operation: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub scanned_nodes: usize,
    pub directory_count: usize,
    pub file_count: usize,
    pub symlink_count: usize,
    pub other_count: usize,
    pub omitted_nodes: usize,
    pub issue_count: usize,
    pub is_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub root: String,
    pub generated_at_unix_ms: u128,
    pub nodes: Vec<SnapshotNode>,
    pub issues: Vec<ScanIssue>,
    pub stats: ScanStats,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}

impl Snapshot {
    // Kept dependency-free so the first CLI can be compiled in restricted environments.
    pub fn to_json_pretty(&self) -> String {
        use std::fmt::Write;

        let mut json = String::from("{\n");
        write!(
            json,
            "  \"root\": {},\n  \"generated_at_unix_ms\": {},\n  \"nodes\": [\n",
            quote(&self.root),
            self.generated_at_unix_ms
        )
        .unwrap();
        for (index, node) in self.nodes.iter().enumerate() {
            let comma = if index + 1 == self.nodes.len() { "" } else { "," };
            writeln!(
                json,
                "    {{\"path\": {}, \"name\": {}, \"kind\": {}, \"depth\": {}, \"size\": {}, \"modified_unix_ms\": {}, \"children\": {}, \"child_count\": {}, \"truncated\": {}}}{}",
                quote(&node.path),
                quote(&node.name),
                quote(node.kind.as_str()),
                node.depth,
                node.size,
                option_number(node.modified_unix_ms),
                number_array(&node.children),
                node.child_count,
                node.truncated,
                comma
            )
            .unwrap();
        }
        json.push_str("  ],\n  \"issues\": [\n");
        for (index, issue) in self.issues.iter().enumerate() {
            let comma = if index + 1 == self.issues.len() { "" } else { "," };
            writeln!(
                json,
                "    {{\"path\": {}, \"operation\": {}, \"message\": {}}}{}",
                quote(&issue.path),
                quote(&issue.operation),
                quote(&issue.message),
                comma
            )
            .unwrap();
        }
        write!(
            json,
            "  ],\n  \"stats\": {{\n    \"scanned_nodes\": {},\n    \"directory_count\": {},\n    \"file_count\": {},\n    \"symlink_count\": {},\n    \"other_count\": {},\n    \"omitted_nodes\": {},\n    \"issue_count\": {},\n    \"is_truncated\": {}\n  }}\n}}\n",
            self.stats.scanned_nodes,
            self.stats.directory_count,
            self.stats.file_count,
            self.stats.symlink_count,
            self.stats.other_count,
            self.stats.omitted_nodes,
            self.stats.issue_count,
            self.stats.is_truncated
        )
        .unwrap();
        json
    }
}

fn quote(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                write_control_escape(&mut output, character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn write_control_escape(output: &mut String, code: u32) {
    use std::fmt::Write;
    write!(output, "\\u{code:04x}").unwrap();
}

fn option_number(value: Option<u128>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_owned())
}

fn number_array(values: &[usize]) -> String {
    let values = values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}
