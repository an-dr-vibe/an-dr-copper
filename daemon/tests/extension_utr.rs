use copperd::descriptor::{Descriptor, Permission};
use copperd::schema::parse_and_validate;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn extensions_root() -> PathBuf {
    repo_root().join("extensions")
}

fn extension_dir(id: &str) -> PathBuf {
    extensions_root().join(id)
}

fn read_descriptor(extension_id: &str) -> Descriptor {
    let path = extension_dir(extension_id).join("manifest.json");
    let raw = fs::read_to_string(&path).expect("read descriptor");
    parse_and_validate(&raw).expect("descriptor should be valid")
}

fn read_main_ts(extension_id: &str) -> String {
    let path = extension_dir(extension_id).join("main.ts");
    fs::read_to_string(path).expect("read main.ts")
}

fn extension_folders(root: &Path) -> Vec<PathBuf> {
    let mut result = fs::read_dir(root)
        .expect("read extensions directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    result.sort();
    result
}

#[test]
fn every_extension_has_valid_descriptor_and_main() {
    for ext in extension_folders(&extensions_root()) {
        let descriptor_path = ext.join("manifest.json");
        let main_path = ext.join("main.ts");
        assert!(
            descriptor_path.exists(),
            "missing manifest.json in {}",
            ext.display()
        );
        assert!(main_path.exists(), "missing main.ts in {}", ext.display());

        let raw = fs::read_to_string(&descriptor_path).expect("read descriptor");
        let descriptor = parse_and_validate(&raw).expect("descriptor validation");
        assert!(
            !descriptor.actions.is_empty(),
            "descriptor has no actions in {}",
            descriptor_path.display()
        );
    }
}

#[test]
fn desktop_torrent_descriptor_matches_required_contract() {
    let descriptor = read_descriptor("desktop-torrent-organizer");
    assert_eq!(descriptor.id, "desktop-torrent-organizer");
    assert_eq!(descriptor.trigger, "desktop-torrents");

    assert_eq!(
        descriptor.permissions,
        vec![
            Permission::Fs,
            Permission::Shell,
            Permission::Store,
            Permission::Ui
        ]
    );

    let action_ids = descriptor
        .actions
        .iter()
        .map(|action| action.id.as_str())
        .collect::<Vec<_>>();
    assert!(action_ids.contains(&"move-torrents"));
    assert!(action_ids.contains(&"add-extension"));
    assert!(action_ids.contains(&"show-config"));

    let desktop_input = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "desktopFolder")
        .expect("desktopFolder input");
    assert_eq!(
        desktop_input.default.as_str(),
        Some("~/Desktop"),
        "desktop default should target Desktop"
    );

    let torrents_input = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "torrentsFolder")
        .expect("torrentsFolder input");
    assert_eq!(torrents_input.default.as_str(), Some("~/Desktop/Torrents"));

    let auto_run_input = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "autoRun")
        .expect("autoRun input");
    assert_eq!(auto_run_input.default.as_bool(), Some(true));

    let poll_input = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "pollIntervalSeconds")
        .expect("pollIntervalSeconds input");
    assert_eq!(poll_input.default.as_u64(), Some(5));
}

#[test]
fn desktop_torrent_main_enforces_torrent_only_moves_and_no_delete() {
    let main_ts = read_main_ts("desktop-torrent-organizer");
    assert!(
        main_ts.contains("endsWith(\".torrent\")"),
        "extension should target .torrent files only"
    );
    assert!(
        main_ts.contains("api.fs.move"),
        "extension should move files to Torrents folder"
    );
    assert!(
        !main_ts.contains("api.fs.delete"),
        "extension must not delete files"
    );
    assert!(
        main_ts.contains("extensionsInstallDir"),
        "extension should support package install target directory"
    );
}
