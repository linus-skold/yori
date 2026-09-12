pub fn greeting() -> &'static str {
    "Hey there"
}

pub fn timeout_seconds() -> u64 {
    45
}

pub fn validate(name: &str) -> bool {
    // Reject whitespace-only names.
    !name.trim().is_empty()
}

pub fn retries() -> usize {
    3
}
