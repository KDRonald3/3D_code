/// Fully public: reachable as `text_engine::format::upper`.
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

/// Visible inside `text-engine` only. NOT reachable from another crate,
/// even though the module path to it is public.
pub(crate) fn trim_inner(s: &str) -> String {
    s.trim().to_string()
}

/// Private to this module.
fn never_visible(s: &str) -> usize {
    s.len()
}

/// A public inline module, two levels deep from the dependency's root.
pub mod deep {
    /// Reachable as `text_engine::format::deep::buried`.
    pub fn buried(s: &str) -> String {
        format!("[{s}]")
    }
}

pub fn uses_privates(s: &str) -> usize {
    never_visible(&trim_inner(s))
}
