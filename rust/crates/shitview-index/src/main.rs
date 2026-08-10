use shitview_core::{ScanConfig, Scanner};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") || args.is_empty() {
        print_help();
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    let root = PathBuf::from(&args[0]);
    let max_nodes = option_value(&args, "--max-nodes")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_600);
    let max_children = option_value(&args, "--max-children")
        .and_then(|value| value.parse().ok())
        .unwrap_or(180);
    let output = option_value(&args, "--output").map(PathBuf::from);

    if !root.exists() {
        eprintln!("root does not exist: {}", root.display());
        return ExitCode::from(2);
    }
    if !root.is_dir() {
        eprintln!("root is not a directory: {}", root.display());
        return ExitCode::from(2);
    }

    let started = std::time::Instant::now();
    let scanner = Scanner::new(ScanConfig {
        max_nodes,
        max_children_per_directory: max_children,
        ..ScanConfig::default()
    });
    let snapshot = scanner.scan(&root);
    let json = snapshot.to_json_pretty();

    let write_result = match output {
        Some(path) => fs::write(&path, json).map(|_| format!("wrote {}", path.display())),
        None => {
            println!("{json}");
            Ok(String::from("wrote stdout"))
        }
    };
    if let Err(error) = write_result {
        eprintln!("failed to write snapshot: {error}");
        return ExitCode::from(1);
    }

    eprintln!(
        "indexed {} nodes ({} dirs, {} files, {} omitted, {} issues) in {:.2?}",
        snapshot.stats.scanned_nodes,
        snapshot.stats.directory_count,
        snapshot.stats.file_count,
        snapshot.stats.omitted_nodes,
        snapshot.stats.issue_count,
        started.elapsed()
    );
    ExitCode::SUCCESS
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn print_help() {
    eprintln!(
        "shitview-index <folder> [--output snapshot.json] [--max-nodes N] [--max-children N]"
    );
}
