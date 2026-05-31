#[derive(Debug, Clone)]
pub struct KeyCombo {
    /// Canonical form: modifiers in order (ctrl, alt, shift, cmd) joined by `+`, e.g. "ctrl+alt+f12"
    pub combo: String,
    /// Human-readable label, e.g. "Ctrl + Alt + F12"
    pub label: String,
}

const MODIFIER_ORDER: &[&str] = &["ctrl", "alt", "shift", "cmd"];

/// Normalizes a key combo string to canonical form (ordered modifiers + final key).
/// Input: any `+`-separated combo, e.g. "shift+ctrl+a" → "ctrl+shift+a".
pub fn normalize_combo(combo: &str) -> KeyCombo {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let mut mods: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| MODIFIER_ORDER.contains(p))
        .collect();
    mods.sort_by_key(|m| {
        MODIFIER_ORDER
            .iter()
            .position(|o| o == m)
            .unwrap_or(usize::MAX)
    });

    let final_key = parts
        .iter()
        .copied()
        .find(|p| !MODIFIER_ORDER.contains(p))
        .unwrap_or("");

    let mut all: Vec<&str> = mods.clone();
    if !final_key.is_empty() {
        all.push(final_key);
    }

    let normalized = all.join("+");
    let label = all
        .iter()
        .map(|p| key_label(p))
        .collect::<Vec<_>>()
        .join(" + ");

    KeyCombo {
        combo: normalized,
        label,
    }
}

/// Returns a human-readable label for a single key name.
pub fn key_label(name: &str) -> String {
    match name {
        "ctrl" => "Ctrl".to_string(),
        "alt" => "Alt".to_string(),
        "shift" => "Shift".to_string(),
        "cmd" => "Cmd".to_string(),
        "scroll_lock" => "Scroll Lock".to_string(),
        "caps_lock" => "Caps Lock".to_string(),
        "num_lock" => "Num Lock".to_string(),
        "page_up" => "Page Up".to_string(),
        "page_down" => "Page Down".to_string(),
        "arrow_up" => "Arrow Up".to_string(),
        "arrow_down" => "Arrow Down".to_string(),
        "arrow_left" => "Arrow Left".to_string(),
        "arrow_right" => "Arrow Right".to_string(),
        name if name.starts_with('f') && name[1..].parse::<u8>().is_ok() => name.to_uppercase(),
        name => {
            let mut chars = name.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        }
    }
}

/// Types a string of text at the current cursor position.
pub fn type_text(_text: &str) {}

/// Sends a single key press and release.
pub fn send_key(_key: &str) {}

/// Sends a key combination, e.g. "ctrl+c".
pub fn send_combo(_combo: &str) {}

#[cfg(test)]
mod tests {
    use super::{key_label, normalize_combo, send_combo, send_key, type_text};

    #[test]
    fn normalize_combo_orders_modifiers() {
        let result = normalize_combo("shift+ctrl+a");
        assert_eq!(result.combo, "ctrl+shift+a");
    }

    #[test]
    fn normalize_combo_single_key() {
        let result = normalize_combo("scroll_lock");
        assert_eq!(result.combo, "scroll_lock");
        assert_eq!(result.label, "Scroll Lock");
    }

    #[test]
    fn normalize_combo_label_format() {
        let result = normalize_combo("ctrl+alt+f12");
        assert_eq!(result.combo, "ctrl+alt+f12");
        assert_eq!(result.label, "Ctrl + Alt + F12");
    }

    #[test]
    fn key_label_function_keys() {
        assert_eq!(key_label("f12"), "F12");
        assert_eq!(key_label("f1"), "F1");
    }

    #[test]
    fn key_label_modifiers() {
        assert_eq!(key_label("ctrl"), "Ctrl");
        assert_eq!(key_label("alt"), "Alt");
        assert_eq!(key_label("shift"), "Shift");
    }

    #[test]
    fn type_text_is_noop_and_does_not_panic() {
        type_text("hello world");
    }

    #[test]
    fn send_key_is_noop_and_does_not_panic() {
        send_key("f12");
    }

    #[test]
    fn send_combo_is_noop_and_does_not_panic() {
        send_combo("ctrl+alt+f12");
    }
}
