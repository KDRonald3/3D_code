//! The file is `renamed_on_disk.rs` but the module is `tidy_name`, because
//! `lib.rs` declared it with `#[path = "renamed_on_disk.rs"] pub mod tidy_name;`.
//! Path is `crate::tidy_name`, and no filename anywhere matches that.

pub fn label() -> &'static str {
    "module name != file name"
}
