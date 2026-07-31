//! Counterpart to `glob-ambiguity/`: same two `get` definitions and both
//! globs, plus an explicit `use crate::text::get;` that outranks the globs.

pub mod app;
pub mod shapes;
pub mod text;
