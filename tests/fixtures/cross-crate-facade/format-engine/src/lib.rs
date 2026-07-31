pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

pub fn area(r: f64) -> f64 {
    std::f64::consts::PI * r * r
}

pub fn version() -> &'static str {
    "0.1.0"
}
