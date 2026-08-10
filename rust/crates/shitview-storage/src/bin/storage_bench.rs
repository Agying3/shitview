use rusqlite::Connection;
use shitview_storage::run_synthetic_benchmark;
use std::process::ExitCode;

fn main() -> ExitCode {
    let rows = std::env::args()
        .skip(1)
        .find_map(|value| value.parse::<usize>().ok())
        .unwrap_or(100_000);
    let mut connection = match Connection::open_in_memory() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("failed to open benchmark database: {error}");
            return ExitCode::from(1);
        }
    };
    match run_synthetic_benchmark(&mut connection, rows) {
        Ok(result) => {
            let bytes = result.page_count.saturating_mul(result.page_size);
            println!(
                "rows={} insert_ms={:.2} count_ms={:.3} sqlite_bytes={}",
                result.rows,
                result.insert_elapsed.as_secs_f64() * 1_000.0,
                result.count_elapsed.as_secs_f64() * 1_000.0,
                bytes
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("storage benchmark failed: {error}");
            ExitCode::from(1)
        }
    }
}
