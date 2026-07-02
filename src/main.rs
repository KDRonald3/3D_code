//! Command-line entry point.
//!
//! Scans a codebase directory — or a single source file — and prints the
//! analysis model as JSON, or writes it to a file. The interactive app is
//! served by the `server` binary; this CLI is handy for inspecting what the
//! analyzer produces for a given tree or file.

use std::path::PathBuf;
use std::process;

use codebase_visualizer::{analyze, scan_path};

/// CLI entry point.
///
/// Parses command-line arguments (the target `<PATH>` — a directory or a
/// single source file — plus `-o/--output`, `--pretty`, `--max-file-bytes`,
/// and `-h/--help`), scans it with [`scan_path`], analyzes the result with
/// [`analyze`], and serializes the model to JSON. The JSON is written to the `--output` file when given, otherwise
/// printed to stdout. Exits with a non-zero status on a missing path, an
/// unknown option, or a scan/write failure.
fn main() {
    // Parse command-line arguments (skipping argv[0]) into local options, with
    // defaults for `max_file_bytes` and `pretty`.
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut max_file_bytes: u64 = 500_000;
    let mut pretty = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-o" | "--output" => {
                output = args.next().map(PathBuf::from);
            }
            "--pretty" => pretty = true,
            "--max-file-bytes" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    max_file_bytes = v;
                }
            }
            other if !other.starts_with('-') => path = Some(PathBuf::from(other)),
            other => {
                eprintln!("unknown option: {other}");
                process::exit(2);
            }
        }
    }

    // A target path is required; bail out with usage and a non-zero status if absent.
    let Some(path) = path else {
        eprintln!("error: missing path to scan\n");
        print_help();
        process::exit(2);
    };

    // Scan the file or directory for source files, exiting on I/O failure.
    let (repo_name, files) = match scan_path(&path, max_file_bytes) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: failed to scan {}: {e}", path.display());
            process::exit(1);
        }
    };

    // Analyze the scanned files into the model and serialize it to JSON,
    // honoring the `--pretty` flag for indented output.
    eprintln!("scanned {} source files from {repo_name}", files.len());
    let model = analyze(&repo_name, files);
    let json = if pretty {
        serde_json::to_string_pretty(&model)
    } else {
        serde_json::to_string(&model)
    }
    .expect("serialize model");

    // Choose the output destination: write to the `--output` file when set
    // (exiting on write failure), otherwise print the JSON to stdout.
    match output {
        Some(out) => {
            if let Err(e) = std::fs::write(&out, json) {
                eprintln!("error: failed to write {}: {e}", out.display());
                process::exit(1);
            }
            eprintln!("wrote model to {}", out.display());
        }
        None => println!("{json}"),
    }
}

/// Print CLI usage and options to stderr.
fn print_help() {
    eprintln!(
        "Codebase Visualizer — analyzer CLI\n\n\
         USAGE:\n    codebase_visualizer <PATH> [OPTIONS]\n\n\
         <PATH> may be a project directory or a single source file.\n\n\
         OPTIONS:\n\
         \x20   -o, --output FILE       write model JSON to FILE (default: stdout)\n\
         \x20   --pretty                pretty-print the JSON\n\
         \x20   --max-file-bytes BYTES  skip files larger than this (default: 500000)\n\
         \x20   -h, --help              show this help\n\n\
         To run the interactive app, start the server:\n    cargo run --bin server"
    );
}
