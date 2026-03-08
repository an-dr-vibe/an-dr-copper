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
