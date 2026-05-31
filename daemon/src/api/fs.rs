use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

pub fn list(path: &str) -> Vec<FileEntry> {
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut entries: Vec<FileEntry> = read_dir
        .filter_map(|e| {
            let e = e.ok()?;
            Some(FileEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.path().display().to_string(),
                is_dir: e.file_type().ok()?.is_dir(),
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

pub fn move_file(src: &str, dst: &str) -> Result<(), std::io::Error> {
    std::fs::rename(src, dst)
        .or_else(|_| std::fs::copy(src, dst).and_then(|_| std::fs::remove_file(src)))
}

pub fn delete(path: &str) -> Result<(), std::io::Error> {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

#[cfg(test)]
mod tests {
    use super::{delete, list, move_file, FileEntry};
    use tempfile::tempdir;

    #[test]
    fn list_empty_dir_returns_empty() {
        let dir = tempdir().expect("tempdir");
        let entries = list(dir.path().to_str().unwrap());
        assert!(entries.is_empty());
    }

    #[test]
    fn list_nonexistent_path_returns_empty() {
        assert!(list("/this/path/does/not/exist/at/all").is_empty());
    }

    #[test]
    fn list_returns_files_and_dirs() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "").expect("write");
        std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
        let entries = list(dir.path().to_str().unwrap());
        assert_eq!(entries.len(), 2);
        let file = entries.iter().find(|e| e.name == "a.txt").expect("file");
        assert!(!file.is_dir);
        let sub = entries.iter().find(|e| e.name == "subdir").expect("dir");
        assert!(sub.is_dir);
    }

    #[test]
    fn move_file_renames_file() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");
        std::fs::write(&src, "data").expect("write");
        move_file(src.to_str().unwrap(), dst.to_str().unwrap()).expect("move");
        assert!(dst.exists());
        assert!(!src.exists());
    }

    #[test]
    fn delete_removes_file() {
        let dir = tempdir().expect("tempdir");
        let p = dir.path().join("x.txt");
        std::fs::write(&p, "x").expect("write");
        delete(p.to_str().unwrap()).expect("delete");
        assert!(!p.exists());
    }

    #[test]
    fn file_entry_fields_are_accessible() {
        let entry = FileEntry {
            name: "Desktop".to_string(),
            path: "C:/Users/test/Desktop".to_string(),
            is_dir: true,
        };
        assert_eq!(entry.name, "Desktop");
        assert!(entry.is_dir);
    }
}
