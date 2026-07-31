//! Serialization of the function map to JSON.

use crate::map::Repository;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Serialize `repo` as pretty-printed JSON to any writer.
pub fn write_map<W: Write>(repo: &Repository, mut writer: W) -> Result<()> {
    serde_json::to_writer_pretty(&mut writer, repo).context("failed to serialize function map")?;
    writeln!(writer).context("failed to write trailing newline")?;
    Ok(())
}

/// Serialize `repo` as compact (single-line) JSON to any writer.
pub fn write_map_compact<W: Write>(repo: &Repository, mut writer: W) -> Result<()> {
    serde_json::to_writer(&mut writer, repo).context("failed to serialize function map")?;
    writeln!(writer).context("failed to write trailing newline")?;
    Ok(())
}

/// Serialize `repo` as pretty-printed JSON into a `String`.
pub fn map_to_string(repo: &Repository) -> Result<String> {
    let mut buf = Vec::new();
    write_map(repo, &mut buf)?;
    String::from_utf8(buf).context("JSON output was not valid UTF-8")
}

/// Write `repo` as pretty-printed JSON to `path`.
pub fn write_map_to_file(repo: &Repository, path: &Path) -> Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    write_map(repo, file)
}

/// Write `repo` as compact JSON to `path`.
pub fn write_map_compact_to_file(repo: &Repository, path: &Path) -> Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    write_map_compact(repo, file)
}

/// Deserialize a function map from JSON bytes.
pub fn map_from_slice(bytes: &[u8]) -> Result<Repository> {
    serde_json::from_slice(bytes).context("failed to deserialize function map")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{
        CallSite, CallTarget, Conflict, Crate, Dependency, DependencyKind, DocComment,
        DocCommentKind, File, Folder, Function, FunctionId, MapSummary, Repository,
        UnresolvedCall,
    };
    use std::path::PathBuf;

    fn sample_map() -> Repository {
        let get_shapes = FunctionId::from_parts("text_engine", "crate::shapes::get", None);
        let get_text = FunctionId::from_parts("text_engine", "crate::text::get", None);
        let main_id = FunctionId::from_parts("text_engine", "crate::run", None);

        let mut summary = MapSummary::empty();
        summary.record_conflict();
        summary.record_unresolved();

        Repository {
            root: PathBuf::from("/tmp/example"),
            summary,
            crates: vec![Crate {
                name: "text-engine".into(),
                rustc_name: "text_engine".into(),
                is_library: true,
                edition: "2021".into(),
                roots: vec![PathBuf::from("/tmp/example/src/lib.rs")],
                dependencies: vec![Dependency {
                    name: "serde".into(),
                    rename: None,
                    kind: DependencyKind::External,
                    path: None,
                }],
                folders: vec![Folder {
                    path: PathBuf::from("/tmp/example/src"),
                    folders: vec![],
                    files: vec![
                        File {
                            path: PathBuf::from("/tmp/example/src/shapes.rs"),
                            module_path: "crate::shapes".into(),
                            content_hash: crate::content_hash(b"pub fn get() {}\n"),
                            functions: vec![Function {
                                id: get_shapes.clone(),
                                name: "get".into(),
                                module_path: "crate::shapes::get".into(),
                                line: 12,
                                byte_start: 0,
                                byte_end: 16,
                                call_sites: vec![],
                                doc_comments: vec![DocComment {
                                    kind: DocCommentKind::Outer,
                                    text: "Fetch a shape by id.".into(),
                                }],
                            }],
                            call_sites: vec![],
                            doc_comments: vec![],
                        },
                        File {
                            path: PathBuf::from("/tmp/example/src/text.rs"),
                            module_path: "crate::text".into(),
                            content_hash: crate::content_hash(b"pub fn get() {}\n"),
                            functions: vec![Function {
                                id: get_text.clone(),
                                name: "get".into(),
                                module_path: "crate::text::get".into(),
                                line: 8,
                                byte_start: 0,
                                byte_end: 16,
                                call_sites: vec![],
                                doc_comments: vec![],
                            }],
                            call_sites: vec![],
                            doc_comments: vec![],
                        },
                    ],
                }],
                files: vec![File {
                    path: PathBuf::from("/tmp/example/src/lib.rs"),
                    module_path: "crate".into(),
                    content_hash: crate::content_hash(b"fn run() {}\n"),
                    functions: vec![Function {
                        id: main_id,
                        name: "run".into(),
                        module_path: "crate::run".into(),
                        line: 3,
                        byte_start: 0,
                        byte_end: 12,
                        call_sites: vec![
                            CallSite {
                                call_path: "shapes::get".into(),
                                line: 5,
                                byte_start: 64,
                                byte_end: 75,
                                target: CallTarget::Resolved(get_shapes.clone()),
                                from_macro: false,
                            },
                            CallSite {
                                call_path: "get".into(),
                                line: 6,
                                byte_start: 90,
                                byte_end: 93,
                                target: CallTarget::Conflict(Conflict {
                                    candidates: vec![get_shapes, get_text],
                                    reason: "ambiguous glob imports from shapes and text".into(),
                                }),
                                from_macro: false,
                            },
                            CallSite {
                                call_path: "mystery".into(),
                                line: 7,
                                byte_start: 108,
                                byte_end: 115,
                                target: CallTarget::Unresolved(UnresolvedCall {
                                    reason: "no indexed definition matches `mystery`".into(),
                                }),
                                from_macro: false,
                            },
                        ],
                        doc_comments: vec![],
                    }],
                    call_sites: vec![],
                    doc_comments: vec![],
                }],
            }],
        }
    }

    #[test]
    fn sample_map_json_round_trips() {
        let original = sample_map();
        let json = map_to_string(&original).expect("serialize");
        let restored = map_from_slice(json.as_bytes()).expect("deserialize");
        assert_eq!(original, restored);

        // Spot-check the public shape a frontend would see.
        assert!(json.contains("\"kind\": \"resolved\""));
        assert!(json.contains("\"kind\": \"conflict\""));
        assert!(json.contains("\"kind\": \"unresolved\""));
        assert!(json.contains("text_engine::shapes::get"));
        assert!(json.contains("ambiguous glob imports"));
        assert!(json.contains("no indexed definition matches `mystery`"));
        assert!(json.contains("\"byte_start\": 64"));
        assert!(json.contains("\"call_sites\""));
        assert!(json.contains("\"content_hash\""));
        assert!(json.contains("\"byte_end\": 12"));
    }

    #[test]
    fn function_id_from_parts_matches_format() {
        let unique = FunctionId::from_parts("text_engine", "crate::shapes::get", None);
        assert_eq!(unique.as_str(), "text_engine::shapes::get");

        let root = FunctionId::from_parts("text_engine", "crate::run", None);
        assert_eq!(root.as_str(), "text_engine::run");

        let bin = FunctionId::from_parts("horizon[bin]", "crate::main", None);
        assert_eq!(bin.as_str(), "horizon[bin]::main");

        let id = FunctionId::from_parts("fs_utils", "crate::open", Some(10));
        assert_eq!(id.as_str(), "fs_utils::open#L10");
        let other = FunctionId::from_parts("fs_utils", "crate::open", Some(14));
        assert_eq!(other.as_str(), "fs_utils::open#L14");
        assert_ne!(id, other);
    }

    #[test]
    fn each_call_target_variant_has_stable_json_shape() {
        let resolved = serde_json::to_value(CallTarget::Resolved(FunctionId::from_parts(
            "text_engine",
            "crate::shapes::get",
            None,
        )))
        .unwrap();
        assert_eq!(
            resolved,
            serde_json::json!({
                "kind": "resolved",
                "data": "text_engine::shapes::get"
            })
        );

        let conflict = serde_json::to_value(CallTarget::Conflict(Conflict {
            candidates: vec![
                FunctionId::from_parts("text_engine", "crate::shapes::get", None),
                FunctionId::from_parts("text_engine", "crate::text::get", None),
            ],
            reason: "ambiguous glob imports from shapes and text".into(),
        }))
        .unwrap();
        assert_eq!(
            conflict,
            serde_json::json!({
                "kind": "conflict",
                "data": {
                    "candidates": [
                        "text_engine::shapes::get",
                        "text_engine::text::get"
                    ],
                    "reason": "ambiguous glob imports from shapes and text"
                }
            })
        );

        let unresolved = serde_json::to_value(CallTarget::Unresolved(UnresolvedCall {
            reason: "no indexed definition matches `mystery`".into(),
        }))
        .unwrap();
        assert_eq!(
            unresolved,
            serde_json::json!({
                "kind": "unresolved",
                "data": {
                    "reason": "no indexed definition matches `mystery`"
                }
            })
        );
    }
}
