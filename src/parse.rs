//! Stage 4: thin wrapper over `ra_ap_syntax`.
//!
//! Parses source text at the crate's real edition. Error-tolerant: broken
//! mid-edit code still yields a syntax tree.

use anyhow::Result;
use ra_ap_syntax::{Edition, SourceFile};
use std::str::FromStr;

/// Edition string as reported by cargo metadata (`"2015"`, `"2018"`, `"2021"`,
/// `"2024"`).
pub type EditionStr = String;

/// Parse `source` into a rust-analyzer syntax tree at `edition`.
///
/// Always returns a tree. Parse errors are not fatal — `ra_ap_syntax` builds a
/// usable CST even for incomplete or broken input.
pub fn parse_source(source: &str, edition: &EditionStr) -> Result<SourceFile> {
    let edition = Edition::from_str(edition).unwrap_or(Edition::CURRENT);
    // `Parse::tree` always succeeds for SourceFile; errors live on the Parse
    // value and are intentionally ignored so mid-edit code still maps.
    Ok(SourceFile::parse(source, edition).tree())
}
