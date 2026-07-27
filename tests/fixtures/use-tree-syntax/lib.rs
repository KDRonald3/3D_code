//! Exercises the awkward corners of `use` tree syntax.

pub mod alpha;
pub mod beta;

// nested brace list: two levels of UseTreeList
use crate::{
    alpha::{one, two as second},
    beta::three,
};

// `self` in a brace list binds the module itself, not a name called "self"
use crate::alpha::{self, four};

// `as _` binds nothing nameable
use crate::beta::Marker as _;

// deep path, single import
use crate::beta::deep::buried;

// a dependency, which must not be confused for one of ours
use std::collections::HashMap;

pub fn drive() -> usize {
    let a = one();
    let b = second();
    let c = three();
    let d = four();
    let e = buried();
    let f = alpha::one();
    let mut m = HashMap::new();
    m.insert(a, b);
    a + b + c + d + e + f
}
