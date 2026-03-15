pub fn console_output_enabled() -> bool {
    std::env::var("COPPER_NO_CONSOLE")
        .map(|value| value != "1")
        .unwrap_or(true)
}

pub fn info(message: impl std::fmt::Display) {
    if console_output_enabled() {
        println!("{message}");
    }
}

pub fn error(message: impl std::fmt::Display) {
    if console_output_enabled() {
        eprintln!("{message}");
    }
}
