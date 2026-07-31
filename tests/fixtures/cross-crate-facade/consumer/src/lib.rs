//! Consumer of a facade crate. Calls only `text_facade::…` names.

use text_facade::{area, chained, from_private, split, upper};

pub fn run(s: &str) -> String {
    let a = upper(s);
    let b = split(s);
    let c = chained();
    let d = area(2.0);
    let e = from_private();
    // `pub extern crate format_engine as fmt_eng` module-prefix facade.
    let f = text_facade::fmt_eng::upper(s);
    // Glob conflict at the facade root — must be Conflict, never a pick.
    let g = text_facade::get(1);
    // Negatives: pub(crate) re-export and private module path.
    let _ = text_facade::crate_only_version();
    let _ = text_facade::private_mod::from_private();
    // Dependency gate: consumer does not declare format-engine.
    let _ = format_engine::upper(s);
    format!("{a}/{b}/{c}/{d}/{e}/{f}/{g}")
}
