use copperd::descriptor::{Descriptor, Permission, RuntimeKind, COMPONENT_ABI_V1};
use copperd::extension::Registry;
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
fn shipped_extension_contract_matrix_is_stable() {
    let expected = [
        (
            "desktop-torrent-organizer",
            "desktop-torrents",
            vec![
                Permission::Fs,
                Permission::Shell,
                Permission::Store,
                Permission::Ui,
            ],
            vec!["move-torrents", "show-config"],
        ),
        (
            "safe-input-key",
            "safe-input-key",
            vec![
                Permission::Keyboard,
                Permission::SecureStore,
                Permission::Store,
                Permission::Ui,
            ],
            vec!["type-text", "setup", "clear"],
        ),
        (
            "session-counter",
            "session-count",
            vec![Permission::Store, Permission::Ui],
            vec!["increment"],
        ),
        (
            "sort-downloads",
            "sort-dl",
            vec![Permission::Fs, Permission::Ui],
            vec!["sort"],
        ),
        (
            "windows-display-manager",
            "windows-display",
            vec![
                Permission::Ui,
                Permission::Store,
                Permission::WindowsDisplay,
            ],
            vec![
                "status",
                "toggle-taskbar-autohide",
                "set-taskbar-autohide",
                "set-resolution",
                "set-scale",
            ],
        ),
    ];

    assert_eq!(
        extension_folders(&extensions_root()).len(),
        expected.len(),
        "adding or removing a shipped extension requires an explicit parity decision"
    );

    for (id, trigger, permissions, action_ids) in expected {
        let descriptor = read_descriptor(id);
        assert!(
            descriptor.runtime.is_none(),
            "{id} should stay on the compatibility runtime until its WASM port lands"
        );
        assert_eq!(descriptor.trigger, trigger, "{id} trigger changed");
        assert_eq!(
            descriptor.permissions, permissions,
            "{id} permissions changed"
        );
        assert_eq!(
            descriptor
                .actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            action_ids,
            "{id} actions changed"
        );
    }
}

#[test]
fn wasm_component_manifest_resolves_an_id_matched_artifact_without_main_ts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let extension = temp.path().join("counter");
    fs::create_dir_all(&extension).expect("extension directory");
    fs::write(
        extension.join("manifest.json"),
        format!(
            r#"{{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "counter",
                "name": "Counter",
                "version": "1.0.0",
                "trigger": "counter",
                "runtime": {{
                    "kind": "wasm-component",
                    "abi": "{COMPONENT_ABI_V1}",
                    "artifact": "counter.wasm"
                }},
                "actions": [{{ "id": "increment", "label": "Increment", "script": "increment" }}]
            }}"#
        ),
    )
    .expect("manifest");
    fs::write(extension.join("counter.wasm"), b"\0asm").expect("artifact");

    let registry = Registry::load_from_dir(temp.path()).expect("registry");
    let loaded = registry.get("counter").expect("counter");
    let runtime = loaded.descriptor.runtime.as_ref().expect("runtime");
    assert_eq!(runtime.kind, RuntimeKind::WasmComponent);
    assert_eq!(runtime.abi, COMPONENT_ABI_V1);
    assert_eq!(
        loaded
            .wasm_component_path()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str()),
        Some("counter.wasm")
    );
}

#[test]
fn wasm_component_manifest_rejects_an_artifact_not_named_for_its_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let extension = temp.path().join("counter");
    fs::create_dir_all(&extension).expect("extension directory");
    fs::write(
        extension.join("manifest.json"),
        format!(
            r#"{{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "counter",
                "name": "Counter",
                "version": "1.0.0",
                "trigger": "counter",
                "runtime": {{
                    "kind": "wasm-component",
                    "abi": "{COMPONENT_ABI_V1}",
                    "artifact": "other.wasm"
                }},
                "actions": [{{ "id": "increment", "label": "Increment", "script": "increment" }}]
            }}"#
        ),
    )
    .expect("manifest");
    fs::write(extension.join("other.wasm"), b"\0asm").expect("artifact");

    let error = Registry::load_from_dir(temp.path()).expect_err("identity mismatch");
    assert!(error.to_string().contains("must be named counter.wasm"));
}

#[test]
fn wasm_component_manifest_rejects_a_directory_as_its_artifact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let extension = temp.path().join("counter");
    fs::create_dir_all(extension.join("counter.wasm")).expect("artifact directory");
    fs::write(
        extension.join("manifest.json"),
        format!(
            r#"{{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "counter",
                "name": "Counter",
                "version": "1.0.0",
                "trigger": "counter",
                "runtime": {{
                    "kind": "wasm-component",
                    "abi": "{COMPONENT_ABI_V1}",
                    "artifact": "counter.wasm"
                }},
                "actions": [{{ "id": "increment", "label": "Increment", "script": "increment" }}]
            }}"#
        ),
    )
    .expect("manifest");

    let error = Registry::load_from_dir(temp.path()).expect_err("artifact must be a file");
    assert!(error.to_string().contains("is not a file"));
}

#[test]
fn legacy_typescript_entrypoints_expose_trigger_handlers() {
    for extension in extension_folders(&extensions_root()) {
        let id = extension
            .file_name()
            .expect("extension folder name")
            .to_string_lossy();
        let source = read_main_ts(&id);
        assert!(
            source.contains("export default function"),
            "{id} should export the Copper TypeScript factory"
        );
        assert!(
            source.contains("onTrigger"),
            "{id} should expose the legacy trigger handler"
        );
    }
}

#[test]
fn every_extension_has_valid_descriptor_and_runtime_artifact() {
    for ext in extension_folders(&extensions_root()) {
        let descriptor_path = ext.join("manifest.json");
        assert!(
            descriptor_path.exists(),
            "missing manifest.json in {}",
            ext.display()
        );

        let raw = fs::read_to_string(&descriptor_path).expect("read descriptor");
        let descriptor = parse_and_validate(&raw).expect("descriptor validation");
        let artifact_path = descriptor
            .runtime
            .as_ref()
            .map(|runtime| ext.join(&runtime.artifact))
            .unwrap_or_else(|| ext.join("main.ts"));
        assert!(
            artifact_path.exists(),
            "missing runtime artifact {}",
            artifact_path.display()
        );
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
    assert!(action_ids.contains(&"show-config"));
    assert!(
        !action_ids.contains(&"add-extension"),
        "package install should not live in the torrent organizer"
    );

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
    assert!(
        descriptor
            .inputs
            .iter()
            .all(|input| input.id != "extensionPackage"),
        "package install input should not live in the torrent organizer"
    );
    assert!(
        descriptor
            .inputs
            .iter()
            .all(|input| input.id != "extensionsInstallDir"),
        "extension install directory should not live in the torrent organizer"
    );

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
    assert_eq!(settings.tabs.len(), 2);
    assert_eq!(settings.tabs[0].id, "monitor");
    assert_eq!(settings.tabs[0].sections, vec!["monitor"]);
    assert!(settings.tabs[1].show_status);
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
        !main_ts.contains("extensionsInstallDir"),
        "torrent organizer should not own extension package install settings"
    );
    assert!(
        !main_ts.contains("add-extension"),
        "torrent organizer should not expose package install actions"
    );
}

#[test]
fn windows_display_manager_descriptor_matches_required_contract() {
    let descriptor = read_descriptor("windows-display-manager");
    assert_eq!(descriptor.id, "windows-display-manager");
    assert_eq!(descriptor.trigger, "windows-display");
    let platforms = descriptor
        .platforms
        .iter()
        .map(|platform| platform.as_str())
        .collect::<Vec<_>>();
    assert_eq!(platforms, vec!["windows"]);
    assert_eq!(
        descriptor.permissions,
        vec![
            Permission::Ui,
            Permission::Store,
            Permission::WindowsDisplay
        ]
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

    let resolution_mode = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "resolutionMode")
        .expect("resolutionMode input");
    assert_eq!(
        resolution_mode.field_type,
        copperd::descriptor::InputType::ListSelect
    );
    assert_eq!(resolution_mode.default.as_str(), Some("1920x1080@60"));
    assert_eq!(
        resolution_mode.options_source.as_deref(),
        Some("dynamicOptions.resolutionModes")
    );

    let scale = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "scalePercent")
        .expect("scalePercent input");
    assert_eq!(scale.field_type, copperd::descriptor::InputType::Select);
    assert_eq!(scale.default.as_u64(), Some(100));
    assert_eq!(
        scale.options_source.as_deref(),
        Some("dynamicOptions.scalePercentages")
    );

    let tray_presets = descriptor
        .inputs
        .iter()
        .find(|input| input.id == "trayResolutionPresets")
        .expect("trayResolutionPresets input");
    assert_eq!(tray_presets.label, "Visible resolutions");
    assert_eq!(
        tray_presets.default.as_array().map(|values| values.len()),
        Some(2),
        "windows display manager should seed tray menu presets"
    );
    assert_eq!(
        tray_presets.options_source.as_deref(),
        Some("dynamicOptions.resolutionModes")
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
    assert_eq!(settings.tabs.len(), 4);
    assert_eq!(settings.tabs[0].id, "taskbar");
    assert_eq!(settings.tabs[1].sections, vec!["resolution", "scale"]);
    assert!(settings.tabs[3].show_status);
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
    let resolution_section = settings
        .sections
        .iter()
        .find(|section| section.id == "resolution")
        .expect("resolution section");
    assert_eq!(resolution_section.inputs, vec!["resolutionMode"]);
    let scale_section = settings
        .sections
        .iter()
        .find(|section| section.id == "scale")
        .expect("scale section");
    assert_eq!(scale_section.inputs, vec!["scalePercent"]);
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

#[test]
fn descriptors_can_restrict_supported_platforms() {
    let raw = r#"{
        "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
        "id": "windows-only-ext",
        "name": "Windows Only",
        "version": "1.0.0",
        "trigger": "windows-only",
        "platforms": ["windows"],
        "actions": [
            { "id": "run", "label": "Run", "script": "return;" }
        ]
    }"#;

    let descriptor = parse_and_validate(raw).expect("descriptor validation");
    let platforms = descriptor
        .platforms
        .iter()
        .map(|platform| platform.as_str())
        .collect::<Vec<_>>();
    assert_eq!(platforms, vec!["windows"]);
}
