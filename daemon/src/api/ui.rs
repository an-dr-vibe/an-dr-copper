use serde_json::Value;

pub fn show(_markup: &Value) {}

#[cfg(test)]
mod tests {
    use super::show;

    #[test]
    fn show_is_noop_and_accepts_markup() {
        show(&serde_json::json!({ "type": "toast", "text": "ok" }));
    }
}
