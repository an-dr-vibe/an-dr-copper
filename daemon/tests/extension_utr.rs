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

    let settings = descriptor
        .settings
        .expect("desktop torrent settings metadata");
    assert_eq!(
        settings.description.as_deref(),
        Some("Configure how Copper watches the desktop for incoming .torrent files.")
    );
    assert!(
        settings
            .sections
            .iter()
            .any(|section| section.id == "monitor"),
        "desktop torrent settings should define a monitor section"
    );
    assert!(
        !settings
            .sections
            .iter()
            .any(|section| section.id == "package-install"),
        "package install settings should live in core configuration, not in the torrent extension"
    );
    assert!(
        settings
            .status
            .as_ref()
            .map(|status| status
                .fields
                .iter()
                .any(|field| field.key == "lastScanUnix"))
            .unwrap_or(false),
        "desktop torrent settings should describe status fields"
    );
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

#[test]
fn windows_display_manager_descriptor_matches_required_contract() {
    let descriptor = read_descriptor("windows-display-manager");
    assert_eq!(descriptor.id, "windows-display-manager");
    assert_eq!(descriptor.trigger, "windows-display");
    assert_eq!(
        descriptor.permissions,
        vec![Permission::Ui, Permission::Store]
    );

    let action_ids = descriptor
        .actions
        .iter()
        .map(|action| action.id.as_str())
        .collect::<Vec<_>>();
    assert!(action_ids.contains(&"status"));
    assert!(action_ids.contains(&"toggle-taskbar-autohide"));
    assert!(action_ids.contains(&"set-taskbar-autohide"));
    assert!(action_ids.contains(&"set-resolution"));
    assert!(action_ids.contains(&"set-scale"));

    let width = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "resolutionWidth")
        .expect("resolutionWidth input");
    assert_eq!(width.default.as_u64(), Some(1920));

    let height = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "resolutionHeight")
        .expect("resolutionHeight input");
    assert_eq!(height.default.as_u64(), Some(1080));

    let hz = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "refreshRate")
        .expect("refreshRate input");
    assert_eq!(hz.default.as_u64(), Some(60));

    let scale = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "scalePercent")
        .expect("scalePercent input");
    assert_eq!(scale.default.as_u64(), Some(100));

    let tray_presets = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "trayResolutionPresets")
        .expect("trayResolutionPresets input");
    assert_eq!(
        tray_presets.default.as_array().map(|values| values.len()),
        Some(2),
        "windows display manager should seed tray menu presets"
    );
    assert_eq!(
        tray_presets.options_source.as_deref(),
        Some("dynamicOptions.trayResolutionPresets")
    );

    let settings = descriptor
        .settings
        .expect("windows display settings metadata");
    assert_eq!(settings.title.as_deref(), Some("Display"));
    assert_eq!(
        settings.apply_actions,
        vec![
            "set-taskbar-autohide".to_string(),
            "set-resolution".to_string(),
            "set-scale".to_string()
        ],
        "windows display settings should declare which actions apply saved settings"
    );
    assert!(
        settings
            .sections
            .iter()
            .any(|section| section.id == "taskbar"),
        "windows display settings should define a taskbar section"
    );
    assert!(
        settings
            .sections
            .iter()
            .any(|section| section.id == "tray-menu"),
        "windows display settings should define a tray menu section"
    );
    assert!(
        settings
            .status
            .as_ref()
            .map(|status| status
                .fields
                .iter()
                .any(|field| field.key == "lastActionUnix"))
            .unwrap_or(false),
        "windows display settings should describe status fields"
    );
    let tray = descriptor
        .tray
        .expect("windows display manager should declare tray metadata");
    assert_eq!(tray.provider, "windows-display");
    assert_eq!(tray.title, "Windows Display Manager");
}

#[test]
fn windows_display_manager_main_documents_host_api_contract() {
    let main_ts = read_main_ts("windows-display-manager");
    assert!(
        main_ts.contains("windows-display-manager"),
        "extension should identify itself"
    );
    assert!(
        main_ts.contains("toggle-taskbar-autohide"),
        "extension should expose taskbar toggle action"
    );
    assert!(
        main_ts.contains("set-resolution"),
        "extension should expose resolution action"
    );
    assert!(
        main_ts.contains("set-scale"),
        "extension should expose scale action"
    );
    assert!(
        main_ts.contains("daemon trigger"),
        "extension should document trigger entrypoint"
    );
}
