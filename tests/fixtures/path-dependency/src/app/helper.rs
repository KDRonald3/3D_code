//! Declared by `mod helper;` inside `app.rs`, NOT in `lib.rs`.
//! Because the declaring module is `app`, the file must live in `src/app/`.
//! Its path is `crate::app::helper`.

pub fn tag(s: &str) -> String {
    format!("<{s}>")
}

/// Reaches the crate root's sibling module WITHOUT `crate::`.
/// `super` here is `app`, and `super::super` is the crate root.
pub fn via_super(s: &str) -> String {
    super::super::text::upper(s)
}
