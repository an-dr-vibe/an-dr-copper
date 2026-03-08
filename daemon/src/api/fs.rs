#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

pub fn list(_path: &str) -> Vec<FileEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{list, FileEntry};

    #[test]
    fn list_returns_empty_by_default() {
        let entries = list("C:/tmp");
        assert!(entries.is_empty());
    }

    #[test]
    fn file_entry_fields_are_accessible() {
        let entry = FileEntry {
            name: "Desktop".to_string(),
            path: "C:/Users/test/Desktop".to_string(),
            is_dir: true,
        };
        assert_eq!(entry.name, "Desktop");
        assert!(entry.path.ends_with("Desktop"));
        assert!(entry.is_dir);
    }
}
