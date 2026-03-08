#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

pub fn list(_path: &str) -> Vec<FileEntry> {
    Vec::new()
}
