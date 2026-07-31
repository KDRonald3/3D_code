//! Type definitions and inherent-method analysis for Horizon.
//!
//! This crate owns everything that was temporarily mixed into the free-function
//! map algorithm: struct/enum/trait/alias extraction, field/alias `type_refs`,
//! inherent `impl Type` methods, and one-hop method-call resolution.
//!
//! Discovery, module walking, and parsing are reused from [`horizon_engine`].
//! The free-function map (`horizon_engine::build_function_map`) stays free of
//! types and methods — see the crate README for why that boundary exists.

pub mod extract;
pub mod map;
pub mod pipeline;
pub mod resolve;

pub use map::{
    CallSite, CallTarget, Conflict, Crate, Dependency, DependencyKind, DocComment, DocCommentKind,
    File, Folder, Function, FunctionId, MapSummary, Repository, TypeConflict, TypeId, TypeItem,
    TypeKind, TypeRef, TypeTarget, UnresolvedCall, UnresolvedType,
};

use anyhow::Result;
use extract::{CallOwnerKind, FileFacts, PendingCall, assign_type_ids};
use horizon_engine::discover;
use pipeline::{extract_repository, resolve_index_for};
use resolve::{ExclusionKind, ResolveResult, resolve_call, resolve_type_mention};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Analyse `repo_root` for type definitions, inherent methods, and method calls.
///
/// Reuses crate discovery and the `mod` walk from [`horizon_engine`], then runs
/// the type/method extract→resolve pipeline owned by this crate.
pub fn build_type_map(repo_root: impl AsRef<Path>) -> Result<Repository> {
    let root = discover::normalize_path(repo_root.as_ref());
    let extracted = extract_repository(&root)?;

    let mut summary = MapSummary::empty();
    let mut crates = Vec::with_capacity(extracted.len());

    for i in 0..extracted.len() {
        let index = resolve_index_for(&extracted, i);
        let types_for_path = build_types_for_crate(&extracted[i], &index);

        let type_id_by_path: HashMap<String, TypeId> = types_for_path
            .values()
            .flatten()
            .map(|t| (t.module_path.clone(), t.id.clone()))
            .collect();

        let mut built_files = Vec::new();
        for (path, module_path, facts) in &extracted[i].file_facts {
            let (mut functions, file_calls) =
                attach_resolved_calls(facts.clone(), &index, &mut summary)?;
            for func in &mut functions {
                if let Some(ty_path) = facts.method_receivers.get(&func.id) {
                    func.receiver_type = type_id_by_path.get(ty_path).cloned();
                }
            }
            let types = types_for_path.get(path).cloned().unwrap_or_default();
            built_files.push(File {
                path: path.clone(),
                module_path: module_path.clone(),
                content_hash: facts.content_hash.clone(),
                functions,
                types,
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

        let src = &extracted[i].krate;
        crates.push(Crate {
            name: src.name.clone(),
            rustc_name: src.rustc_name.clone(),
            is_library: src.is_library,
            edition: src.edition.clone(),
            roots: src.roots.clone(),
            dependencies: src
                .dependencies
                .iter()
                .map(|d| Dependency {
                    name: d.name.clone(),
                    rename: d.rename.clone(),
                    kind: match d.kind {
                        horizon_map::DependencyKind::Path => DependencyKind::Path,
                        horizon_map::DependencyKind::External => DependencyKind::External,
                    },
                    path: d.path.clone(),
                })
                .collect(),
            folders,
            files: root_files,
        });
    }

    Ok(Repository {
        root,
        crates,
        summary,
    })
}

fn build_types_for_crate(
    extracted: &pipeline::ExtractedCrate,
    index: &resolve::ResolveIndex,
) -> HashMap<PathBuf, Vec<TypeItem>> {
    let id_prefix = extracted.krate.function_id_prefix();
    let mut type_items = assign_type_ids(&id_prefix, &extracted.types);
    let id_by_full_path: HashMap<String, TypeId> = type_items
        .iter()
        .map(|t| (t.module_path.clone(), t.id.clone()))
        .collect();

    for (item, src) in type_items.iter_mut().zip(extracted.types.iter()) {
        let mut refs = Vec::new();
        for pending in &src.pending_refs {
            let Some(target) = resolve_type_mention(
                &pending.type_path,
                &src.module_path,
                index,
                &id_by_full_path,
            ) else {
                continue;
            };
            refs.push(TypeRef {
                type_path: pending.type_path.clone(),
                line: pending.line,
                byte_start: pending.byte_start,
                byte_end: pending.byte_end,
                target,
            });
        }
        item.type_refs = refs;
    }

    let mut by_key: HashMap<String, Vec<TypeItem>> = HashMap::new();
    for (item, src) in type_items.into_iter().zip(extracted.types.iter()) {
        let key = format!("{}@@{}@@{}", src.module_path, src.name, src.line);
        by_key.entry(key).or_default().push(item);
    }

    let mut types_for_path: HashMap<PathBuf, Vec<TypeItem>> = HashMap::new();
    for (path, _module_path, facts) in &extracted.file_facts {
        let mut file_types = Vec::new();
        for ty in &facts.types {
            let key = format!("{}@@{}@@{}", ty.module_path, ty.name, ty.line);
            if let Some(mut items) = by_key.remove(&key) {
                file_types.append(&mut items);
            }
        }
        types_for_path.insert(path.clone(), file_types);
    }
    types_for_path
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
