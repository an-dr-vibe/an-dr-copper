use keyring::Entry;

/// Retrieves a secret from the OS keychain.
/// Returns `None` if the entry does not exist or cannot be read.
pub fn get(service: &str, key: &str) -> Option<String> {
    Entry::new(service, key)
        .ok()
        .and_then(|e| e.get_password().ok())
}

/// Stores a secret in the OS keychain.
/// On Windows: Credential Manager. On Linux: SecretService (GNOME Keyring / KDE Wallet).
/// Silently no-ops if the keychain is unavailable.
pub fn set(service: &str, key: &str, value: &str) {
    if let Ok(entry) = Entry::new(service, key) {
        let _ = entry.set_password(value);
    }
}

/// Removes a secret from the OS keychain. No-op if the entry does not exist.
pub fn delete(service: &str, key: &str) {
    if let Ok(entry) = Entry::new(service, key) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::{delete, get, set};

    #[test]
    fn roundtrip_set_get_delete() {
        let service = "copper-test";
        let key = "roundtrip-key";
        let value = "test-secret";

        set(service, key, value);
        assert_eq!(get(service, key).as_deref(), Some(value));
        delete(service, key);
        assert!(get(service, key).is_none());
    }

    #[test]
    fn get_returns_none_for_missing_entry() {
        assert!(get("copper-test", "nonexistent-key-xyz").is_none());
    }

    #[test]
    fn delete_is_noop_for_missing_entry() {
        delete("copper-test", "nonexistent-key-xyz");
    }
}
