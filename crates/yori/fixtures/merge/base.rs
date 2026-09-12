pub fn greeting() -> &'static str {
    "Hello"
}

pub fn timeout_seconds() -> u64 {
    30
}

pub fn legacy_mode() -> bool {
    true
}

pub fn validate(name: &str) -> bool {
    !name.is_empty()
}

pub fn retries() -> usize {
    3
}
