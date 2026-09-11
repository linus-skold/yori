fn shared(value: usize) -> usize {
    value + 1
}

fn removed_helper() {
    println!("removed");
}

fn greet(name: &str) {
	println!("Hello, {name}!");
}
