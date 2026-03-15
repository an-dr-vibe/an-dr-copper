use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn daemon_src(file_name: &str) -> PathBuf {
    repo_root().join("daemon").join("src").join(file_name)
}

fn production_source(file_name: &str) -> String {
    let raw = fs::read_to_string(daemon_src(file_name)).expect("read source");
    raw.split("\n#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or(&raw)
        .to_string()
}

#[test]
fn shipped_extension_ids_are_confined_to_registry_and_tests() {
    let guarded_files = ["daemon.rs", "config_ui.rs", "cli.rs"];
    let forbidden_ids = [
        "desktop-torrent-organizer",
        "session-counter",
        "windows-display-manager",
    ];

    for file_name in guarded_files {
        let source = production_source(file_name);
        for forbidden_id in forbidden_ids {
            assert!(
                !source.contains(forbidden_id),
                "production source {file_name} should not hardcode shipped extension id {forbidden_id}"
            );
        }
    }
}
