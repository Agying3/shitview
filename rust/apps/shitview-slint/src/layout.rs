use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const LAYOUT_VERSION: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutEntry {
    pub x: f32,
    pub y: f32,
    pub pinned: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LayoutStore {
    file_path: Option<PathBuf>,
    entries: HashMap<String, LayoutEntry>,
}

impl LayoutStore {
    pub fn load(root: &Path) -> (Self, Option<String>) {
        let file_path = root.join(".shitview").join("layout.json");
        let mut store = Self {
            file_path: Some(file_path.clone()),
            entries: HashMap::new(),
        };
        let contents = match fs::read_to_string(&file_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (store, None),
            Err(error) => {
                return (
                    store,
                    Some(format!("Cannot read layout file: {error}")),
                )
            }
        };
        let value: Value = match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                return (store, Some(format!("Ignoring invalid layout JSON: {error}")))
            }
        };
        let Some(nodes) = value.get("nodes").and_then(Value::as_object) else {
            return (store, Some("Ignoring layout file without a nodes object".to_owned()));
        };
        for (key, value) in nodes {
            if let Some(entry) = parse_entry(value) {
                store.entries.insert(normalize_key(key), entry);
            }
        }
        (store, None)
    }

    pub fn entry_for(&self, stable_id: Option<&[u8]>, path: &str) -> Option<LayoutEntry> {
        stable_id
            .map(|id| layout_key(Some(id), path))
            .and_then(|key| self.entries.get(&key).copied())
            .or_else(|| self.entries.get(&layout_key(None, path)).copied())
    }

    pub fn set(&mut self, stable_id: Option<&[u8]>, path: &str, entry: LayoutEntry) {
        self.entries
            .insert(layout_key(stable_id, path), sanitize_entry(entry));
    }

    pub fn remove(&mut self, stable_id: Option<&[u8]>, path: &str) {
        self.entries.remove(&layout_key(stable_id, path));
        if stable_id.is_some() {
            self.entries.remove(&layout_key(None, path));
        }
    }

    pub fn remove_all(&mut self) {
        self.entries.clear();
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(file_path) = &self.file_path else {
            return Ok(());
        };
        let parent = file_path
            .parent()
            .ok_or_else(|| std::io::Error::other("layout file has no parent"))?;
        fs::create_dir_all(parent)?;

        let mut nodes = Map::new();
        let mut keys = self.entries.keys().cloned().collect::<Vec<_>>();
        keys.sort_unstable();
        for key in keys {
            let entry = self.entries[&key];
            nodes.insert(
                key,
                serde_json::json!({
                    "x": entry.x,
                    "y": entry.y,
                    "pinned": entry.pinned,
                }),
            );
        }
        let document = serde_json::json!({
            "version": LAYOUT_VERSION,
            "nodes": nodes,
        });
        let temporary = file_path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&document)?)?;
        match fs::rename(&temporary, file_path) {
            Ok(()) => Ok(()),
            Err(rename_error) => {
                // Windows cannot replace an existing file with rename. Keep the
                // normal path atomic, with a replace fallback for that platform.
                if file_path.exists() {
                    fs::remove_file(file_path)?;
                    fs::rename(&temporary, file_path)
                } else {
                    Err(rename_error)
                }
            }
        }
    }

    #[cfg(test)]
    fn with_entries(entries: HashMap<String, LayoutEntry>) -> Self {
        Self {
            file_path: None,
            entries,
        }
    }
}

pub fn layout_key(stable_id: Option<&[u8]>, path: &str) -> String {
    stable_id
        .map(|id| format!("id:{}", hex_encode(id)))
        .unwrap_or_else(|| normalize_key(path))
}

fn parse_entry(value: &Value) -> Option<LayoutEntry> {
    if let Some(values) = value.as_array() {
        return Some(sanitize_entry(LayoutEntry {
            x: values.first()?.as_f64()? as f32,
            y: values.get(1)?.as_f64()? as f32,
            pinned: false,
        }));
    }
    let object = value.as_object()?;
    Some(sanitize_entry(LayoutEntry {
        x: object.get("x")?.as_f64()? as f32,
        y: object.get("y")?.as_f64()? as f32,
        pinned: object.get("pinned").and_then(Value::as_bool).unwrap_or(false),
    }))
}

fn sanitize_entry(entry: LayoutEntry) -> LayoutEntry {
    LayoutEntry {
        x: if entry.x.is_finite() { entry.x } else { 0.0 },
        y: if entry.y.is_finite() { entry.y } else { 0.0 },
        pinned: entry.pinned,
    }
}

fn normalize_key(key: &str) -> String {
    if key.starts_with("id:") {
        key.to_owned()
    } else {
        key.replace('\\', "/")
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{layout_key, LayoutEntry, LayoutStore};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "shitview-layout-{label}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn reads_legacy_array_entries() {
        let root = temp_root("legacy");
        fs::create_dir_all(root.join(".shitview")).unwrap();
        fs::write(
            root.join(".shitview/layout.json"),
            r#"{"version":3,"nodes":{"H:\\project\\main.rs":[12.5, 8.0]}}"#,
        )
        .unwrap();
        let (store, warning) = LayoutStore::load(&root);
        assert!(warning.is_none());
        assert_eq!(
            store.entry_for(None, "H:/project/main.rs"),
            Some(LayoutEntry { x: 12.5, y: 8.0, pinned: false })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stable_id_takes_priority_over_path_fallback() {
        let id = [1_u8, 2, 3];
        let mut entries = HashMap::new();
        entries.insert(
            layout_key(None, "H:/project/main.rs"),
            LayoutEntry { x: 1.0, y: 2.0, pinned: false },
        );
        entries.insert(
            layout_key(Some(&id), "H:/project/main.rs"),
            LayoutEntry { x: 3.0, y: 4.0, pinned: true },
        );
        let store = LayoutStore::with_entries(entries);
        assert_eq!(store.entry_for(Some(&id), "H:/project/main.rs").unwrap().x, 3.0);
        assert!(store.entry_for(Some(&id), "H:/project/main.rs").unwrap().pinned);
    }

    #[test]
    fn saves_and_restores_pinned_entries() {
        let root = temp_root("save");
        let id = [9_u8, 8, 7];
        let (mut store, warning) = LayoutStore::load(&root);
        assert!(warning.is_none());
        store.set(
            Some(&id),
            "H:/project/main.rs",
            LayoutEntry { x: 444.0, y: 333.0, pinned: true },
        );
        store.save().unwrap();
        let contents = fs::read_to_string(root.join(".shitview/layout.json")).unwrap();
        assert!(contents.contains("\"version\": 4"));
        let (restored, warning) = LayoutStore::load(&root);
        assert!(warning.is_none());
        assert_eq!(
            restored.entry_for(Some(&id), "H:/renamed/main.rs"),
            Some(LayoutEntry { x: 444.0, y: 333.0, pinned: true })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_json_is_a_non_fatal_warning() {
        let root = temp_root("invalid");
        fs::create_dir_all(root.join(".shitview")).unwrap();
        fs::write(root.join(".shitview/layout.json"), "{ invalid").unwrap();
        let (store, warning) = LayoutStore::load(&root);
        assert!(warning.unwrap().contains("invalid layout JSON"));
        assert!(store.entry_for(None, "H:/project/main.rs").is_none());
        let _ = fs::remove_dir_all(root);
    }
}
