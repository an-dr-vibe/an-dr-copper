pub fn notify(_message: &str) {}

#[cfg(test)]
mod tests {
    use super::notify;

    #[test]
    fn notify_is_noop_and_does_not_panic() {
        notify("hello");
    }
}
