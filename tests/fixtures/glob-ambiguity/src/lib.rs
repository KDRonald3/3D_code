//! Minimal crate whose sole purpose is to trigger rustc `E0659`:
//! two sibling modules each define `get`, both are glob-imported, and
//! `get` is called unqualified.

pub mod app;
pub mod shapes;
pub mod text;
