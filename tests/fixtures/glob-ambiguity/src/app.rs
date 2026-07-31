//! Both globs, no explicit import — rustc rejects `get` as ambiguous (`E0659`).

use crate::shapes::*;
use crate::text::*;

pub fn run() -> usize {
    get(3)
}
