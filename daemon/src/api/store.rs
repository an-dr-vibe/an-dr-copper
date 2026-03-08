use std::collections::BTreeMap;

#[derive(Default)]
pub struct InMemoryStore {
    data: BTreeMap<String, serde_json::Value>,
}

impl InMemoryStore {
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.data.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryStore;

    #[test]
    fn set_and_get_roundtrip() {
        let mut store = InMemoryStore::default();
        store.set("answer".to_string(), serde_json::json!(42));
        assert_eq!(store.get("answer").and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn set_overwrites_existing_value() {
        let mut store = InMemoryStore::default();
        store.set("key".to_string(), serde_json::json!("old"));
        store.set("key".to_string(), serde_json::json!("new"));
        assert_eq!(store.get("key").and_then(|v| v.as_str()), Some("new"));
    }
}
