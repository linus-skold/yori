pub fn greeting() -> &'static str {
    "Welcome back"
}

pub fn timeout_seconds() -> u64 {
    30
}

pub fn legacy_mode() -> bool {
    // Keep compatibility, but disable it by default.
    false
}

pub fn validate(name: &str) -> bool {
    // Limit the size of user-provided names.
    !name.is_empty() && name.len() <= 80
}

pub fn retries() -> usize {
    5
}
