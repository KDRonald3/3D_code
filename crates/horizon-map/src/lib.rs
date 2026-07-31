//! Horizon function-map JSON contract: node types and serde helpers.
//!
//! This crate deliberately stays free of `ra_ap_syntax` so consumers can load a
//! saved map (and re-hash a file for staleness) without compiling the analyser.
//! Dependencies are `serde` / `serde_json` / `anyhow` / `sha2`.

pub mod hash;
pub mod json;
pub mod map;

pub use hash::content_hash;
pub use json::{
    map_from_slice, map_to_string, write_map, write_map_compact, write_map_compact_to_file,
    write_map_to_file,
};
pub use map::{
    CallSite, CallTarget, Conflict, Crate, Dependency, DependencyKind, DocComment, DocCommentKind,
    File, Folder, Function, FunctionId, MapSummary, Repository, UnresolvedCall,
};
