use std::borrow::Cow;

fn shared(value: usize) -> usize {
    value + 1
}

fn greet(name: &str) {
	println!("Hello, {name} 👋");
}

fn added_helper() -> Cow<'static, str> {
    Cow::Borrowed("added")
}
