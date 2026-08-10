use rusqlite::{params, Connection, Result};
use std::time::{Duration, Instant};

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
    use super::{initialize_schema, run_synthetic_benchmark};
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
}
