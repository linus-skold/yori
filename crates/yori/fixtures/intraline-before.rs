fn retry_delay(customer_count: usize, attempt: usize) -> usize {
    let base_delay = 1000;
    if attempt >= 3 {
        return base_delay;
    }
    base_delay + customer_count
}

fn request() {
    send_request(customer, timeout);
}

fn status() -> &'static str {
    // Temporary status message for the retry screen.
    "Waiting for the customer"
}

fn greeting() -> &'static str {
    "Hello 👩‍💻"
}
