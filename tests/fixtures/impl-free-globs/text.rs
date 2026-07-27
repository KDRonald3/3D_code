//! Text helpers, plus a NESTED inline module two levels from the crate root.

pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

/// Within-file call down into the nested module.
pub fn shout(s: &str) -> String {
    case::snake(&upper(s))
}

pub mod case {
    /// Full path is `crate::text::case::snake`.
    pub fn snake(s: &str) -> String {
        s.replace(' ', "_")
    }

    /// Calls up to the parent module.
    pub fn kebab(s: &str) -> String {
        super::upper(s).replace(' ', "-")
    }
}
