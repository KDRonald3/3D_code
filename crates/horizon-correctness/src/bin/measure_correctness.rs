//! Re-runnable correctness harness for Horizon's function map.
//!
//! ```text
//! cargo run --bin measure_correctness -- fixtures
//! cargo run --bin measure_correctness -- self --lsif target/correctness/horizon.lsif
//! cargo run --bin measure_correctness -- exclusions .
//! ```
//!
//! See `docs/correctness-measurement.md`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use horizon_correctness::{
    ExclusionAudit, FixtureOracle, LsifReport, OracleReport, audit_exclusions, check_oracle,
    collect_call_outcomes, compare_lsif, load_oracle,
};
use horizon_engine::{build_function_map, map_to_string};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is nested under crates/<name>")
        .to_path_buf()
}

fn default_fixtures_dir() -> PathBuf {
    workspace_root().join("tests/fixtures")
}

#[derive(Debug, Parser)]
#[command(
    name = "measure_correctness",
    about = "Measure Horizon function-map correctness (fixtures, LSIF, exclusions)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run hand-annotated fixture oracles (and the adversarial fixture).
    Fixtures {
        /// Directory containing fixture crates (default: tests/fixtures).
        #[arg(long, default_value_os_t = default_fixtures_dir())]
        fixtures_dir: PathBuf,
        /// Write a JSON report to this path.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Map a repository and optionally compare resolved edges to an LSIF index.
    SelfMap {
        /// Repository root (default: .).
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Existing LSIF file from `rust-analyzer lsif`.
        #[arg(long)]
        lsif: Option<PathBuf>,
        /// Generate LSIF with rust-analyzer before comparing.
        #[arg(long)]
        generate_lsif: bool,
        /// Where to write / read the LSIF file when generating.
        #[arg(long, default_value = "target/correctness/horizon.lsif")]
        lsif_out: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Dump deliberate-exclusion samples and heuristic suspicious drops.
    Exclusions {
        /// Repository root (default: .).
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Samples per exclusion category.
        #[arg(long, default_value_t = 12)]
        sample: usize,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Run fixtures + self-map exclusions (no LSIF unless --lsif is passed).
    All {
        #[arg(long, default_value_os_t = default_fixtures_dir())]
        fixtures_dir: PathBuf,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        lsif: Option<PathBuf>,
        #[arg(long)]
        generate_lsif: bool,
        #[arg(long, default_value = "target/correctness/horizon.lsif")]
        lsif_out: PathBuf,
        #[arg(short, long, default_value = "target/correctness/report.json")]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Fixtures {
            fixtures_dir,
            output,
        } => {
            let report = run_fixtures(&fixtures_dir)?;
            print_fixture_summary(&report);
            if let Some(path) = output {
                write_json(&path, &report)?;
            }
            if report.iter().any(|r| r.passed != r.checked) {
                bail!("fixture oracle failures");
            }
        }
        Cmd::SelfMap {
            repo,
            lsif,
            generate_lsif,
            lsif_out,
            output,
        } => {
            let report = run_self_map(&repo, lsif.as_deref(), generate_lsif, &lsif_out)?;
            print_self_summary(&report);
            if let Some(path) = output {
                write_json(&path, &report)?;
            }
            if !report.lsif.false_positives.is_empty() {
                bail!(
                    "{} LSIF false positives (regenerate LSIF atomically with the map)",
                    report.lsif.false_positives.len()
                );
            }
        }
        Cmd::Exclusions {
            repo,
            sample,
            output,
        } => {
            let audit = run_exclusions(&repo, sample)?;
            print_exclusion_summary(&audit);
            if let Some(path) = output {
                write_json(&path, &audit)?;
            }
            if !audit.suspicious.is_empty() {
                eprintln!(
                    "warning: {} heuristically suspicious exclusions (inspect samples)",
                    audit.suspicious.len()
                );
            }
        }
        Cmd::All {
            fixtures_dir,
            repo,
            lsif,
            generate_lsif,
            lsif_out,
            output,
        } => {
            let fixtures = run_fixtures(&fixtures_dir)?;
            let exclusions = run_exclusions(&repo, 12)?;
            let self_map = run_self_map(&repo, lsif.as_deref(), generate_lsif, &lsif_out)?;
            print_fixture_summary(&fixtures);
            print_exclusion_summary(&exclusions);
            print_self_summary(&self_map);
            let combined = serde_json::json!({
                "fixtures": fixtures,
                "exclusions": exclusions,
                "self_map": self_map,
            });
            write_json(&output, &combined)?;
            if fixtures.iter().any(|r| r.passed != r.checked) {
                bail!("fixture oracle failures");
            }
            if !self_map.lsif.false_positives.is_empty() {
                bail!("LSIF false positives");
            }
        }
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct SelfMapReport {
    summary: horizon_map::MapSummary,
    resolved_edges: usize,
    conflicts: usize,
    unresolved: usize,
    lsif: LsifReport,
}

fn run_fixtures(fixtures_dir: &Path) -> Result<Vec<OracleReport>> {
    let mut reports = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_dir)
        .with_context(|| format!("read {}", fixtures_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join("expected-edges.json").is_file())
        .collect();
    dirs.sort();

    if dirs.is_empty() {
        bail!("no expected-edges.json under {}", fixtures_dir.display());
    }

    for dir in dirs {
        let oracle: FixtureOracle = load_oracle(&dir)?;
        let map = build_function_map(&dir)
            .with_context(|| format!("build map for {}", dir.display()))?;
        // Ensure JSON still serializes.
        let _ = map_to_string(&map)?;
        reports.push(check_oracle(&map, &oracle));
    }
    Ok(reports)
}

fn run_self_map(
    repo: &Path,
    lsif: Option<&Path>,
    generate: bool,
    lsif_out: &Path,
) -> Result<SelfMapReport> {
    let map = build_function_map(repo)?;
    let mut resolved = 0usize;
    let mut conflicts = 0usize;
    let mut unresolved = 0usize;
    for krate in &map.crates {
        let mut files: Vec<&horizon_map::File> = krate.files.iter().collect();
        let mut stack: Vec<&horizon_map::Folder> = krate.folders.iter().collect();
        while let Some(folder) = stack.pop() {
            files.extend(folder.files.iter());
            stack.extend(folder.folders.iter());
        }
        for f in files {
            for func in &f.functions {
                for site in &func.call_sites {
                    match &site.target {
                        horizon_map::CallTarget::Resolved(_) => resolved += 1,
                        horizon_map::CallTarget::Conflict(_) => conflicts += 1,
                        horizon_map::CallTarget::Unresolved(_) => unresolved += 1,
                    }
                }
            }
            for site in &f.call_sites {
                match &site.target {
                    horizon_map::CallTarget::Resolved(_) => resolved += 1,
                    horizon_map::CallTarget::Conflict(_) => conflicts += 1,
                    horizon_map::CallTarget::Unresolved(_) => unresolved += 1,
                }
            }
        }
    }

    let lsif_path = if generate {
        if let Some(parent) = lsif_out.parent() {
            fs::create_dir_all(parent)?;
        }
        eprintln!("generating LSIF via rust-analyzer → {}", lsif_out.display());
        let status = Command::new("rust-analyzer")
            .args([
                "lsif",
                &repo.to_string_lossy(),
                "--exclude-vendored-libraries",
            ])
            .stdout(fs::File::create(lsif_out)?)
            .status()
            .context("run rust-analyzer lsif (is rust-analyzer installed?)")?;
        if !status.success() {
            bail!("rust-analyzer lsif failed with {status}");
        }
        Some(lsif_out.to_path_buf())
    } else {
        lsif.map(|p| p.to_path_buf())
    };

    let lsif_report = if let Some(path) = lsif_path {
        compare_lsif(&map, &path)?
    } else {
        LsifReport::default()
    };

    Ok(SelfMapReport {
        summary: map.summary.clone(),
        resolved_edges: resolved,
        conflicts,
        unresolved,
        lsif: lsif_report,
    })
}

fn run_exclusions(repo: &Path, sample: usize) -> Result<ExclusionAudit> {
    let outcomes = collect_call_outcomes(repo)?;
    Ok(audit_exclusions(&outcomes, sample))
}

fn print_fixture_summary(reports: &[OracleReport]) {
    println!("=== Fixture oracles ===");
    let mut checked = 0usize;
    let mut passed = 0usize;
    let mut fps = 0usize;
    for r in reports {
        checked += r.checked;
        passed += r.passed;
        fps += r.false_positives.len();
        let status = if r.passed == r.checked { "ok" } else { "FAIL" };
        println!(
            "  [{status}] {}  {}/{}  fp={} fn={} other={}",
            r.fixture,
            r.passed,
            r.checked,
            r.false_positives.len(),
            r.false_negatives.len(),
            r.other_mismatches.len()
        );
        for m in r
            .false_positives
            .iter()
            .chain(r.false_negatives.iter())
            .chain(r.other_mismatches.iter())
        {
            println!(
                "      {} {} {:?}  expected={}  actual={}",
                m.caller, m.call_path, m.line, m.expected, m.actual
            );
        }
    }
    println!(
        "  total {passed}/{checked}  false_positives={fps}"
    );
}

fn print_self_summary(report: &SelfMapReport) {
    println!("=== Self-map ===");
    println!(
        "  summary: conflicts={} unresolved={} external={} constructor={} associated={} local={}",
        report.summary.conflicts,
        report.summary.unresolved,
        report.summary.external_dropped,
        report.summary.constructor_dropped,
        report.summary.associated_dropped,
        report.summary.local_dropped
    );
    println!(
        "  edges: resolved={} conflicts={} unresolved={}",
        report.resolved_edges, report.conflicts, report.unresolved
    );
    if report.lsif.compared > 0 {
        let rate = report.lsif.false_positives.len() as f64 / report.lsif.compared as f64;
        println!(
            "  LSIF: compared={} matched={} fp={} impl_only={} unmatched={}  fp_rate={rate:.4}",
            report.lsif.compared,
            report.lsif.matched,
            report.lsif.false_positives.len(),
            report.lsif.lsif_impl_only.len(),
            report.lsif.unmatched.len()
        );
        print_lsif_cohort("ordinary", &report.lsif.ordinary);
        print_lsif_cohort("from_macro", &report.lsif.from_macro);
        for m in &report.lsif.false_positives {
            println!(
                "      FP {}:{} {}  H={}  L={}",
                m.file, m.line, m.call_path, m.horizon, m.lsif
            );
        }
    } else {
        println!("  LSIF: (skipped — pass --lsif or --generate-lsif)");
    }
}

fn print_lsif_cohort(label: &str, c: &horizon_correctness::LsifCohort) {
    if c.compared == 0 {
        println!("  LSIF {label}: (none)");
        return;
    }
    let rate = c.false_positives.len() as f64 / c.compared as f64;
    println!(
        "  LSIF {label}: compared={} matched={} fp={} unmatched={}  fp_rate={rate:.4}",
        c.compared,
        c.matched,
        c.false_positives.len(),
        c.unmatched.len()
    );
    for m in &c.false_positives {
        println!(
            "      FP {}:{} {}  H={}  L={}",
            m.file, m.line, m.call_path, m.horizon, m.lsif
        );
    }
}

fn print_exclusion_summary(audit: &ExclusionAudit) {
    println!("=== Exclusion audit ===");
    println!(
        "  counts: external={} constructor={} associated={} local={} suspicious={}",
        audit.external_dropped,
        audit.constructor_dropped,
        audit.associated_dropped,
        audit.local_dropped,
        audit.suspicious.len()
    );
    for (label, samples) in [
        ("external", &audit.samples.external),
        ("constructor", &audit.samples.constructor),
        ("associated", &audit.samples.associated),
        ("local", &audit.samples.local),
    ] {
        println!("  sample {label}:");
        for s in samples {
            println!(
                "      {}:{} {}  ({:?})",
                s.file.display(),
                s.line,
                s.call_path,
                s.kind
            );
        }
    }
    if !audit.suspicious.is_empty() {
        println!("  suspicious:");
        for s in &audit.suspicious {
            println!(
                "      {}:{} {}  ({:?})",
                s.file.display(),
                s.line,
                s.call_path,
                s.kind
            );
        }
    }
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, text + "\n")?;
    eprintln!("wrote {}", path.display());
    Ok(())
}
