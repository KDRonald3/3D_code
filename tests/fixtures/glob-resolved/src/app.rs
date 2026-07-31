//! Same as `glob-ambiguity`, plus an explicit import that wins over globs.

use crate::shapes::*;
use crate::text::*;
use crate::text::get;

pub fn run() -> usize {
    get(3)
}
