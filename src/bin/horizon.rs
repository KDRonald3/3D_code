//! Horizon CLI — thin wrapper over the library.
//!
//! Accepts a repository path and an optional output file (default: stdout).
//! Emits the function map as JSON (pretty-printed by default).

use anyhow::{bail, Context, Result};
use clap::Parser;
use horizon::{
    build_function_map, write_map, write_map_compact, write_map_compact_to_file, write_map_to_file,
    CallTarget, Crate, File, Folder, Repository,
};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "horizon",
    about = "Build a navigable function map of a Rust repository",
    long_about = "Analyse a Rust repository without compiling it and emit a function map as JSON.\n\n\
The map is Repository → Crate → Folder → File → Function. Each free function \
carries outgoing call sites. A call resolves to exactly one definition, or \
records a Conflict naming every candidate, or records Unresolved. Recognised \
out-of-scope calls (external crates, methods, constructors, associated \
functions) are dropped from the tree and counted in the summary.\n\n\
JSON goes to stdout by default (or to --output). A one-line summary is written \
to stderr so piping stdout still yields clean JSON. See docs/json-output.md \
for the full schema."
)]
struct Cli {
    /// Path to the repository root to analyse (directory containing or above Cargo.toml files).
    #[arg(value_name = "REPO")]
    repo: PathBuf,

    /// Write JSON to this file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Emit compact (single-line) JSON instead of pretty-printed.
    #[arg(long)]
    compact: bool,

    /// Suppress the stderr summary line.
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.repo.exists() {
        bail!(
            "path does not exist: {}\n\
             Pass a directory that contains a Rust repository (a Cargo.toml at \
             or under that path).",
            cli.repo.display()
        );
    }

    let map = build_function_map(&cli.repo)
        .with_context(|| format!("failed to analyse {}", cli.repo.display()))?;

    if map.crates.is_empty() && !cli.quiet {
        eprintln!(
            "horizon: warning: no crates found under {} \
             (no Cargo.toml discovered). Emitting an empty map.",
            map.root.display()
        );
    }

    match (&cli.output, cli.compact) {
        (Some(path), false) => write_map_to_file(&map, path)
            .with_context(|| format!("failed to write {}", path.display()))?,
        (Some(path), true) => write_map_compact_to_file(&map, path)
            .with_context(|| format!("failed to write {}", path.display()))?,
        (None, false) => {
            let stdout = io::stdout().lock();
            write_map(&map, BufWriter::new(stdout)).context("failed to write to stdout")?;
        }
        (None, true) => {
            let stdout = io::stdout().lock();
            write_map_compact(&map, BufWriter::new(stdout))
                .context("failed to write to stdout")?;
        }
    }

    if !cli.quiet {
        write_summary_line(&map, io::stderr().lock())?;
    }

    Ok(())
}

fn write_summary_line(map: &Repository, mut err: impl Write) -> Result<()> {
    let stats = count_map(map);
    writeln!(
        err,
        "horizon: {} crate{}, {} function{}; \
         call sites: {} resolved, {} conflict{}, {} unresolved; \
         dropped: {} external, {} constructor, {} associated",
        map.crates.len(),
        plural(map.crates.len()),
        stats.functions,
        plural(stats.functions),
        stats.resolved,
        map.summary.conflicts,
        plural(map.summary.conflicts),
        map.summary.unresolved,
        map.summary.external_dropped,
        map.summary.constructor_dropped,
        map.summary.associated_dropped,
    )
    .context("failed to write summary to stderr")?;
    Ok(())
}

struct TreeStats {
    functions: usize,
    resolved: usize,
}

fn count_map(map: &Repository) -> TreeStats {
    let mut stats = TreeStats {
        functions: 0,
        resolved: 0,
    };
    for krate in &map.crates {
        count_crate(krate, &mut stats);
    }
    stats
}

fn count_crate(krate: &Crate, stats: &mut TreeStats) {
    for file in &krate.files {
        count_file(file, stats);
    }
    for folder in &krate.folders {
        count_folder(folder, stats);
    }
}

fn count_folder(folder: &Folder, stats: &mut TreeStats) {
    for file in &folder.files {
        count_file(file, stats);
    }
    for child in &folder.folders {
        count_folder(child, stats);
    }
}

fn count_file(file: &File, stats: &mut TreeStats) {
    stats.functions += file.functions.len();
    for site in &file.call_sites {
        if matches!(site.target, CallTarget::Resolved(_)) {
            stats.resolved += 1;
        }
    }
    for func in &file.functions {
        for site in &func.call_sites {
            if matches!(site.target, CallTarget::Resolved(_)) {
                stats.resolved += 1;
            }
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
