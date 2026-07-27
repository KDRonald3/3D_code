//! Pure free functions. Zero impls.

/// Unique name across the project.
pub fn sum_all(v: &[f64]) -> f64 {
    v.iter().sum()
}

/// Calls a sibling free function in the SAME file.
pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    sum_all(v) / v.len() as f64
}

/// Cross-file, fully qualified, and nests a within-file call inside the argument.
pub fn report(v: &[f64]) -> String {
    crate::text::upper(&format!("{}", mean(v)))
}
