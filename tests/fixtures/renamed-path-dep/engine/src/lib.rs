//! Path dependency reached under a Cargo rename alias.

pub fn greet(name: &str) -> String {
    format!("hi {name}")
}
