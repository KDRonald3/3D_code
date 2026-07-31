pub fn tokenize(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}
