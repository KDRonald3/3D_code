//! Private module. Outsiders cannot name `text_facade::private_mod::…`,
//! but `pub use private_mod::from_private` at the crate root exposes the fn.

pub fn from_private() -> &'static str {
    "from_private"
}
