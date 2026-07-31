//! Reached only through a glob import, never named in a `use`.

pub fn area(w: f64, h: f64) -> f64 {
    w * h
}

/// Collides with `main.rs::describe`. The local definition there wins.
pub fn describe(s: &str) -> String {
    format!("shape {s}")
}
