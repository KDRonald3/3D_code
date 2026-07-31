//! Discover crates via `cargo metadata`.
//!
//! Finds every `Cargo.toml` under the given root and runs
//! `cargo metadata --no-deps --format-version 1 --offline` per manifest.
//! Yields every mapped compilation unit (library-like targets and binaries),
//! their absolute `src_path` roots, real editions, and dependencies split by
//! whether they carry a `path` key.
//!
//! # Target kinds
//!
//! Included (ordinary Rust source with free functions):
//! - Library-like: `lib`, `rlib`, `dylib`, `cdylib`, `staticlib`, `proc-macro`
//! - Binary: `bin`
//!
//! Deliberately excluded:
//! - `example`, `test`, `bench` — secondary surfaces; inflate maps with
//!   throwaway helpers and hit the same `[dev-dependencies]` blind spot
//!   already recorded elsewhere. Not an accidental omission.
//! - `custom-build` — `build.rs` scripts are build machinery, not product code.
//!
//! # Why `--no-deps` stays
//!
//! `cargo metadata` without `--no-deps` can create or update `Cargo.lock` in
//! the target repository — forbidden, because we analyse trees we do not own.
//! `--no-deps` is read-only. It lists workspace members only, so path
//! dependencies that are not workspace members are followed by recursing into
//! each dependency's `path` directory and running metadata there. Registry and
//! git dependencies are never walked.
//!
//! # Phase 4 — multi-crate
//!
//! - **Workspace members** — every member package contributes its lib/bin targets.
//! - **Path dependencies** — local source, walked via the recursion above.
//! - **Multiple targets** — a package's library-like target (including
//!   `proc-macro`) and each binary become separate [`Crate`] nodes (each with
//!   one root). A binary gets an implicit path dependency on its package's
//!   library when one exists, matching Cargo.
//!
//! # Self-analysis constraint
//!
//! When the analysed root is the Horizon repository itself, crate discovery
//! **must skip `tests/fixtures/`**. Those directories are deliberately broken
//! and deliberately ambiguous sample crates; mapping them as ordinary source
//! would corrupt every self-test of the tool. Implementors of this stage must
//! honour that exclusion — it is not optional.

use horizon_map::{Crate, Dependency, DependencyKind};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Discover all crates under `repo_root`.
///
/// Never refuses: a missing or broken manifest simply contributes no crates.
/// Returns one [`Crate`] per mapped library-like or binary target, sorted
/// stably by `(package name, rustc_name, root path)`.
pub fn discover_crates(repo_root: &Path) -> Result<Vec<Crate>> {
    let manifests = find_manifests(repo_root);
    if manifests.is_empty() {
        return Ok(Vec::new());
    }

    let mut seen_workspaces = HashSet::new();
    let mut seen_targets: HashSet<String> = HashSet::new();
    let mut crates = Vec::new();
    let mut pending_path_manifests: VecDeque<PathBuf> = VecDeque::new();

    for manifest in &manifests {
        pending_path_manifests.push_back(manifest.clone());
    }

    while let Some(manifest) = pending_path_manifests.pop_front() {
        if is_nested_fixture_manifest(&manifest, repo_root) {
            continue;
        }

        let meta = match run_cargo_metadata(&manifest) {
            Ok(m) => m,
            Err(err) => {
                eprintln!(
                    "horizon: skipping manifest {}: {err:#}",
                    manifest.display()
                );
                continue;
            }
        };

        let workspace_key = meta.workspace_root.clone();
        let already_seen_ws = !seen_workspaces.insert(workspace_key);

        let member_ids: HashSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
        for package in meta.packages {
            let is_member = member_ids.contains(package.id.as_str());
            // When we were asked to open a path-dep manifest that Cargo folds
            // into an already-seen workspace, still accept that one package.
            let is_requested_path_pkg = Path::new(&package.manifest_path) == manifest;
            if already_seen_ws && !is_requested_path_pkg {
                continue;
            }
            if !is_member && !is_requested_path_pkg {
                continue;
            }
            if is_nested_fixture_manifest(Path::new(&package.manifest_path), repo_root) {
                continue;
            }

            let package_dir = Path::new(&package.manifest_path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo_root.to_path_buf());

            match crates_from_package(package, &package_dir) {
                Ok(produced) => {
                    for krate in produced {
                        let key = crate_key(&krate);
                        if !seen_targets.insert(key) {
                            continue;
                        }
                        for dep in &krate.dependencies {
                            if dep.kind != DependencyKind::Path {
                                continue;
                            }
                            let Some(dep_path) = dep.path.as_ref() else {
                                continue;
                            };
                            if let Some(dep_manifest) = path_dep_manifest(dep_path) {
                                if !is_nested_fixture_manifest(&dep_manifest, repo_root) {
                                    pending_path_manifests.push_back(dep_manifest);
                                }
                            }
                        }
                        crates.push(krate);
                    }
                }
                Err(err) => {
                    eprintln!("horizon: skipping package: {err:#}");
                }
            }
        }
    }

    crates.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.rustc_name.cmp(&b.rustc_name))
            .then_with(|| a.roots.first().cmp(&b.roots.first()))
    });

    Ok(crates)
}

/// Convert a package name with dashes into the rustc crate identifier.
pub fn rustc_crate_name(package_name: &str) -> String {
    package_name.replace('-', "_")
}

fn crate_key(krate: &Crate) -> String {
    let root = krate
        .roots
        .first()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}|{}|{root}", krate.name, krate.rustc_name)
}

fn path_dep_manifest(dep_path: &Path) -> Option<PathBuf> {
    let manifest = if dep_path.is_file() {
        dep_path.to_path_buf()
    } else {
        dep_path.join("Cargo.toml")
    };
    if manifest.is_file() {
        Some(normalize_path(&manifest))
    } else {
        None
    }
}

/// Canonicalize `path` when possible and strip a Windows `\\?\` verbatim prefix.
///
/// Shared by discovery, module walk, map building, and the correctness harness.
pub fn normalize_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(canonical) => {
            let as_str = canonical.to_string_lossy();
            as_str
                .strip_prefix(r"\\?\")
                .map(PathBuf::from)
                .unwrap_or(canonical)
        }
        Err(_) => path.to_path_buf(),
    }
}

fn find_manifests(repo_root: &Path) -> Vec<PathBuf> {
    let mut manifests = Vec::new();
    let root_manifest = repo_root.join("Cargo.toml");
    if root_manifest.is_file() {
        manifests.push(root_manifest);
    }

    let mut stack = vec![repo_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if is_skipped_dir(&path, repo_root) {
                    continue;
                }
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "Cargo.toml")
                && !manifests.iter().any(|m| m == &path)
                && !is_nested_fixture_manifest(&path, repo_root)
            {
                manifests.push(path);
            }
        }
    }

    manifests.sort();
    manifests
}

fn is_skipped_dir(path: &Path, repo_root: &Path) -> bool {
    if is_nested_fixture_manifest(path, repo_root) {
        return true;
    }
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("target" | ".git" | ".cursor")
    )
}

/// True when `path` lies under `repo_root/tests/fixtures/…`.
///
/// When `repo_root` *is* a fixture crate, its own manifest is not nested and
/// must not be skipped.
fn is_nested_fixture_manifest(path: &Path, repo_root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(repo_root) else {
        return false;
    };
    let mut comps = rel.components();
    matches!(comps.next(), Some(Component::Normal(a)) if a == "tests")
        && matches!(comps.next(), Some(Component::Normal(b)) if b == "fixtures")
}

fn run_cargo_metadata(manifest: &Path) -> Result<Metadata> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .with_context(|| format!("failed to spawn cargo metadata for {}", manifest.display()))?;

    if !output.status.success() {
        // Offline can fail when the lockfile/index is unavailable; retry without
        // --offline. Still use --no-deps so we never write a Cargo.lock.
        let output = Command::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(manifest)
            .output()
            .with_context(|| {
                format!(
                    "failed to spawn cargo metadata (retry) for {}",
                    manifest.display()
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "cargo metadata failed for {}: {stderr}",
                manifest.display()
            );
        }
        return serde_json::from_slice(&output.stdout)
            .context("failed to parse cargo metadata JSON");
    }

    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata JSON")
}

/// Cargo target kinds treated as the package library (one per package).
///
/// `proc-macro` is reported as its own kind (not `lib`) by `cargo metadata`,
/// so omitting it silently drops crates like `serde_derive`. `dylib` /
/// `staticlib` are the same class of library crate-type as `cdylib` / `rlib`.
const LIBRARY_KINDS: &[&str] = &[
    "lib",
    "rlib",
    "dylib",
    "cdylib",
    "staticlib",
    "proc-macro",
];

fn is_library_kind(kinds: &[String]) -> bool {
    kinds.iter().any(|k| LIBRARY_KINDS.contains(&k.as_str()))
}

fn is_bin_kind(kinds: &[String]) -> bool {
    kinds.iter().any(|k| k == "bin")
}

fn crates_from_package(package: MetaPackage, package_dir: &Path) -> Result<Vec<Crate>> {
    let mut lib: Option<MetaTarget> = None;
    let mut bins = Vec::new();

    for target in package.targets {
        // example / test / bench / custom-build are deliberately ignored —
        // see module docs.
        if is_library_kind(&target.kind) {
            lib = Some(target);
        } else if is_bin_kind(&target.kind) {
            bins.push(target);
        }
    }

    if lib.is_none() && bins.is_empty() {
        return Ok(Vec::new());
    }

    let base_dependencies: Vec<Dependency> = package
        .dependencies
        .into_iter()
        .filter(|d| d.kind.as_deref().is_none_or(|k| k == "normal"))
        .map(|d| {
            let (kind, path) = match d.path {
                Some(p) => (DependencyKind::Path, Some(PathBuf::from(p))),
                None => (DependencyKind::External, None),
            };
            Dependency {
                name: d.name,
                rename: d.rename,
                kind,
                path,
            }
        })
        .collect();

    let mut out = Vec::new();

    if let Some(lib_target) = lib.as_ref() {
        out.push(Crate {
            name: package.name.clone(),
            rustc_name: lib_target.name.clone(),
            is_library: true,
            edition: lib_target.edition.clone(),
            roots: vec![PathBuf::from(&lib_target.src_path)],
            dependencies: base_dependencies.clone(),
            folders: Vec::new(),
            files: Vec::new(),
        });
    }

    for bin in &bins {
        let mut dependencies = base_dependencies.clone();
        // Same-package binaries reach their library by package name, as Cargo does.
        if let Some(lib_target) = lib.as_ref() {
            let already = dependencies.iter().any(|d| {
                rustc_crate_name(&d.name) == lib_target.name
                    || d.name == package.name
                    || rustc_crate_name(&d.name) == rustc_crate_name(&package.name)
            });
            if !already {
                dependencies.push(Dependency {
                    name: package.name.clone(),
                    rename: None,
                    kind: DependencyKind::Path,
                    path: Some(package_dir.to_path_buf()),
                });
            }
        }
        // Keep the real cargo target name. Lib+bin name clashes are disambiguated
        // in FunctionId via `Crate::function_id_prefix` (`{name}[bin]`), not by
        // inventing a fake rustc_name such as `horizon_bin`.
        out.push(Crate {
            name: package.name.clone(),
            rustc_name: bin.name.clone(),
            is_library: false,
            edition: bin.edition.clone(),
            roots: vec![PathBuf::from(&bin.src_path)],
            dependencies,
            folders: Vec::new(),
            files: Vec::new(),
        });
    }

    Ok(out)
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<MetaPackage>,
    workspace_members: Vec<String>,
    workspace_root: String,
}

#[derive(Debug, Deserialize)]
struct MetaPackage {
    name: String,
    id: String,
    manifest_path: String,
    dependencies: Vec<MetaDependency>,
    targets: Vec<MetaTarget>,
}

#[derive(Debug, Deserialize)]
struct MetaDependency {
    name: String,
    /// Present when the manifest renames the dependency
    /// (`alias = { package = "real-name", … }`).
    #[serde(default)]
    rename: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetaTarget {
    name: String,
    kind: Vec<String>,
    src_path: String,
    edition: String,
}
