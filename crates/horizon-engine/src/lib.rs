//! Horizon engine — build a navigable function map of a Rust repository.
//!
//! Pipeline (each step in its own module; unnumbered so renames cannot drift):
//!
//! - [`discover`] — `cargo metadata` → crates, roots, dependencies
//! - [`modules`] — `mod` walk → files in the crate and their module paths
//! - [`parse`] — thin wrapper over `ra_ap_syntax`
//! - [`extract`] — per-file facts (definitions, calls, imports, docs)
//! - [`resolve`] — call site → [`horizon_map::CallTarget`] (resolved, conflict, or unresolved)
//! - [`horizon_map`] — node types and JSON emission (separate crate)
//!
//! [`pipeline`] shares extract → resolve-index construction between
//! [`build_function_map`] and the correctness harness so they cannot drift.
//!
//! # Phase 4
//!
//! Multi-crate end-to-end: every workspace member and path-dependency
//! library-like target (including `proc-macro`) plus each binary is walked.
//! Cross-crate edges resolve only along the declared dependency graph, and
//! only to `pub` items behind an all-`pub` module chain. Registry / git
//! dependencies stay dropped.

pub mod discover;
pub mod extract;
pub mod modules;
pub mod parse;
pub mod pipeline;
pub mod resolve;

pub use horizon_map::{
    content_hash, map_from_slice, map_to_string, write_map, write_map_compact,
    write_map_compact_to_file, write_map_to_file,
};
pub use horizon_map::{
    CallSite, CallTarget, Conflict, Crate, Dependency, DependencyKind, DocComment, DocCommentKind,
    File, Folder, Function, FunctionId, MapSummary, Repository, UnresolvedCall,
};

use anyhow::Result;
use extract::{CallOwnerKind, FileFacts, PendingCall};
use pipeline::{extract_repository, resolve_index_for};
use resolve::{ExclusionKind, ResolveResult, resolve_call};
use std::collections::HashMap;
use std::path::Path;

/// Analyse `repo_root` and return its function map.
///
/// Never refuses to produce output: a codebase mid-edit still yields a map
/// (conflicts mark what could not be resolved).
pub fn build_function_map(repo_root: impl AsRef<Path>) -> Result<Repository> {
    let root = discover::normalize_path(repo_root.as_ref());
    let extracted = extract_repository(&root)?;

    let mut summary = MapSummary::empty();
    let mut crates = Vec::with_capacity(extracted.len());

    for i in 0..extracted.len() {
        let index = resolve_index_for(&extracted, i);

        let mut built_files = Vec::new();
        for (path, module_path, facts) in &extracted[i].file_facts {
            let (functions, file_calls) =
                attach_resolved_calls(facts.clone(), &index, &mut summary)?;
            built_files.push(File {
                path: path.clone(),
                module_path: module_path.clone(),
                content_hash: facts.content_hash.clone(),
                functions,
                call_sites: file_calls,
                doc_comments: facts.doc_comments.clone(),
            });
        }

        let src_root = extracted[i]
            .krate
            .roots
            .first()
            .and_then(|r| r.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let (folders, root_files) = build_folder_tree(&src_root, built_files);

        let mut krate = extracted[i].krate.clone();
        krate.folders = folders;
        krate.files = root_files;
        crates.push(krate);
    }

    Ok(Repository {
        root,
        crates,
        summary,
    })
}

fn attach_resolved_calls(
    facts: FileFacts,
    index: &resolve::ResolveIndex,
    summary: &mut MapSummary,
) -> Result<(Vec<Function>, Vec<CallSite>)> {
    let mut calls_by_owner: HashMap<String, Vec<CallSite>> = HashMap::new();
    let mut file_calls = Vec::new();

    for pending in &facts.call_sites {
        let site = resolve_pending(pending, index, summary)?;
        let Some(site) = site else {
            continue;
        };

        match pending.owner {
            CallOwnerKind::Function => {
                if let Some(owner) = pending.enclosing_function.as_ref() {
                    calls_by_owner
                        .entry(owner.as_str().to_string())
                        .or_default()
                        .push(site);
                }
            }
            CallOwnerKind::File => file_calls.push(site),
        }
    }

    let mut functions = facts.functions;
    for func in &mut functions {
        if let Some(sites) = calls_by_owner.remove(func.id.as_str()) {
            func.call_sites = sites;
        }
    }

    Ok((functions, file_calls))
}

fn resolve_pending(
    pending: &PendingCall,
    index: &resolve::ResolveIndex,
    summary: &mut MapSummary,
) -> Result<Option<CallSite>> {
    let target = match resolve_call(pending, index)? {
        ResolveResult::Target(t) => t,
        ResolveResult::External => {
            summary.record_external_dropped();
            return Ok(None);
        }
        ResolveResult::Excluded(ExclusionKind::VariantOrConstructor) => {
            summary.record_constructor_dropped();
            return Ok(None);
        }
        ResolveResult::Excluded(ExclusionKind::AssociatedFunction) => {
            summary.record_associated_dropped();
            return Ok(None);
        }
        ResolveResult::Excluded(ExclusionKind::LocalBinding) => {
            summary.record_local_dropped();
            return Ok(None);
        }
    };

    match &target {
        CallTarget::Conflict(_) => summary.record_conflict(),
        CallTarget::Unresolved(_) => summary.record_unresolved(),
        CallTarget::Resolved(_) => {}
    }

    Ok(Some(CallSite {
        call_path: pending.call_path.clone(),
        line: pending.line,
        byte_start: pending.byte_start,
        byte_end: pending.byte_end,
        target,
        from_macro: pending.from_macro,
    }))
}

/// Place discovered files into `Crate.files` / `Folder` nodes from their paths
/// relative to the crate source root. Only files the module walk found — never
/// a filesystem directory listing.
fn build_folder_tree(src_root: &Path, files: Vec<File>) -> (Vec<Folder>, Vec<File>) {
    let src_root = discover::normalize_path(src_root);
    let mut root_files = Vec::new();
    let mut tree = FolderTree::default();

    for file in files {
        let rel = match file.path.strip_prefix(&src_root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => {
                root_files.push(file);
                continue;
            }
        };
        let parent = rel.parent();
        if parent.is_none() || parent.is_some_and(|p| p.as_os_str().is_empty()) {
            root_files.push(file);
        } else if let Some(parent) = parent {
            tree.insert_file(parent, file);
        }
    }

    root_files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut folders = tree.into_folders(&src_root);
    folders.sort_by(|a, b| a.path.cmp(&b.path));
    (folders, root_files)
}

/// Nested directory → files, built only from module-walk paths.
#[derive(Debug, Default)]
struct FolderTree {
    files: Vec<File>,
    children: HashMap<String, FolderTree>,
}

impl FolderTree {
    fn insert_file(&mut self, rel_dir: &Path, file: File) {
        let mut cursor = self;
        for component in rel_dir.components() {
            let Some(name) = component.as_os_str().to_str() else {
                continue;
            };
            cursor = cursor.children.entry(name.to_string()).or_default();
        }
        cursor.files.push(file);
    }

    fn into_folders(mut self, abs_dir: &Path) -> Vec<Folder> {
        let mut names: Vec<String> = self.children.keys().cloned().collect();
        names.sort();
        let mut folders = Vec::with_capacity(names.len());
        for name in names {
            let child_tree = self.children.remove(&name).unwrap_or_default();
            let path = abs_dir.join(&name);
            let mut files = child_tree.files;
            files.sort_by(|a, b| a.path.cmp(&b.path));
            // Recurse into grandchildren via a fresh tree holding only children.
            let nested = FolderTree {
                files: Vec::new(),
                children: child_tree.children,
            }
            .into_folders(&path);
            folders.push(Folder {
                path,
                folders: nested,
                files,
            });
        }
        folders
    }
}
