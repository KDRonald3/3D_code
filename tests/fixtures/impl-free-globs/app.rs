//! Caller. Exercises every non-impl call form, including the ones that only
//! exist because there is a `lib.rs` crate root above us.

use crate::numbers::{mean, sum_all as total};
use crate::shapes::*;
use crate::text;
use crate::text::*;
use crate::{shout_upper, summarize};

/// Every line below is a distinct call form we have to classify.
pub fn run(values: &[f64], label: &str) -> String {
    // within-file, callee defined LATER in this file
    let n = normalize(label);

    // cross-file, plain import naming the defining module
    let m = mean(values);

    // cross-file through an ALIASED import: written name is not the real name
    let s = total(values);

    // cross-file, module-qualified via `use crate::text`
    let u = text::upper(&n);

    // cross-file, NESTED module path
    let d = text::case::snake(&n);

    // cross-file, fully qualified from the crate root
    let q = crate::numbers::mean(values);

    // through the lib.rs RE-EXPORT: two segments, root-qualified
    let re = crate::mean(values);

    // through a re-export that RENAMED the function
    let rn = shout_upper("x");

    // a function defined in lib.rs itself
    let sm = summarize(values);

    // a lib.rs function, root-qualified
    let wd = crate::width_of(&n);

    // cross-file through a GLOB import, name never written in any `use`
    let a = area(2.0, 3.0);

    // within-file, self-qualified
    let z = self::normalize(label);

    // within-file, INLINE module
    let c = helpers::indent(&n);

    // within-file recursion target
    let r = countdown(3);

    // callee named but NOT called here; passed as a value
    let f = apply(&n, normalize);

    // call sited inside a CLOSURE body
    let g = {
        let k = |x: &str| normalize(x);
        k(label)
    };

    // EXTERNAL crate: must be refused, not guessed
    let e = std::cmp::max(1u32, 2u32);

    let _ = (m, s, u, d, q, re, rn, sm, wd, a, z, c, r, f, g, e);

    // COLLIDES with shapes::describe pulled in by the glob above.
    // Rust rule: an explicit local definition beats a glob import.
    describe(&n)
}

/// Within-file free function. No impl, no receiver.
fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Calls itself.
fn countdown(n: u32) -> u32 {
    if n == 0 {
        0
    } else {
        countdown(n - 1)
    }
}

/// The call happens through a parameter, so no name is available at the site.
fn apply(s: &str, f: fn(&str) -> String) -> String {
    f(s)
}

/// Same name as `shapes::describe`.
fn describe(s: &str) -> String {
    format!("local {s}")
}

/// Contains a nested function, visible only inside this body.
pub fn outer(v: &[f64]) -> f64 {
    fn inner(v: &[f64]) -> f64 {
        v.len() as f64
    }
    inner(v) + mean(v)
}

mod helpers {
    /// Free function inside an inline module.
    pub fn indent(s: &str) -> String {
        format!("  {s}")
    }

    /// Calls OUT of the inline module, up to the parent module.
    pub fn shout(s: &str) -> String {
        super::normalize(s)
    }

    /// Calls all the way up to the crate root.
    pub fn measure(s: &str) -> usize {
        crate::width_of(s)
    }
}
