//! Horizon function-map JSON contract: node types and serde helpers.
//!
//! This crate deliberately depends only on `serde` / `serde_json` / `anyhow`
//! so consumers can load a saved map without compiling `ra_ap_syntax`.

pub mod json;
pub mod map;

pub use json::{
    map_from_slice, map_to_string, write_map, write_map_compact, write_map_compact_to_file,
    write_map_to_file,
};
pub use map::{
    CallSite, CallTarget, Conflict, Crate, Dependency, DependencyKind, DocComment, DocCommentKind,
    File, Folder, Function, FunctionId, MapSummary, Repository, UnresolvedCall,
};
