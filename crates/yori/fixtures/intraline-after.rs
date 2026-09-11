fn retry_delay(customer_total: usize, attempt: usize) -> usize {
    let base_delay = 1500;
    if attempt > 3 {
        return base_delay;
    }
    base_delay + customer_total
}

fn request() {
    send_request(customer, timeout, retry_policy);
}

fn status() -> &'static str {
    // Permanent status message for the retry screen.
    "Waiting for the server"
}

fn greeting() -> &'static str {
    "Hello 👨‍💻"
}
