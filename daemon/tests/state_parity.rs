use copperd::api::store;
use copperd::state_store::ExtensionStateStore;
use std::fs;
use tempfile::tempdir;

const CONFIG_FIXTURE: &str = include_str!("fixtures/state/config.json");
const STATUS_FIXTURE: &str = include_str!("fixtures/state/status.json");
const STORE_FIXTURE: &str = include_str!("fixtures/state/store.json");
const LEGACY_FIXTURE: &str = include_str!("fixtures/state/legacy-data.json");

#[test]
fn representative_extension_state_remains_readable() {
    let temp = tempdir().expect("tempdir");
    let state = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
    let extension_id = "desktop-torrent-organizer";
    let config_path = state.config_path(extension_id);
    let status_path = state.status_path(extension_id);
    let store_path = state.data_root().join(extension_id).join("store.json");
    fs::create_dir_all(config_path.parent().expect("extension directory"))
        .expect("create extension directory");
    fs::write(&config_path, CONFIG_FIXTURE).expect("config fixture");
    fs::write(&status_path, STATUS_FIXTURE).expect("status fixture");
    fs::write(&store_path, STORE_FIXTURE).expect("store fixture");

    let config = state.load_config(extension_id).expect("load config");
    assert_eq!(
        config.get("desktopFolder").and_then(|value| value.as_str()),
        Some("~/Desktop")
    );
    assert_eq!(
        config
            .get("pollIntervalSeconds")
            .and_then(|value| value.as_u64()),
        Some(5)
    );

    let status = state.load_status(extension_id).expect("load status");
    assert_eq!(
        status
            .pointer("/_stateContract/capability")
            .and_then(|value| value.as_str()),
        Some("desktop-torrent-monitor")
    );
    assert_eq!(
        store::get(
            store_path.to_str().expect("UTF-8 store path"),
            "desktop-torrent-organizer/last-run"
        )
        .and_then(|value| value.get("moved").cloned())
        .and_then(|value| value.as_u64()),
        Some(2)
    );
}

#[test]
fn legacy_core_state_remains_a_diagnostic_fallback() {
    let temp = tempdir().expect("tempdir");
    let state = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
    let legacy_path = state.legacy_path("copper-core");
    fs::create_dir_all(legacy_path.parent().expect("core directory"))
        .expect("create core directory");
    fs::write(&legacy_path, LEGACY_FIXTURE).expect("legacy fixture");

    let loaded = state.inspect_core_config().expect("inspect core config");
    assert_eq!(
        loaded
            .value
            .get("disabledExtensions")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(loaded.warnings[0].code, "legacy-path-in-use");
    assert_eq!(loaded.warnings[0].path, legacy_path.display().to_string());
}
