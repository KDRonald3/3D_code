/// `pub` on the function is irrelevant: the MODULE is private, so nothing
/// outside `text-engine` can reach this.
pub fn hidden() -> String {
    "hidden".to_string()
}
