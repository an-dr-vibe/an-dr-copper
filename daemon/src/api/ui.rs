use serde_json::Value;

pub fn show(_markup: &Value) {}

pub fn update(_state: &Value) {}

#[cfg(test)]
mod tests {
    use super::{show, update};

    #[test]
    fn presentation_calls_are_noops_and_accept_structured_values() {
        show(&serde_json::json!({ "type": "toast", "text": "ok" }));
        update(&serde_json::json!({ "progress": 1 }));
    }
}
