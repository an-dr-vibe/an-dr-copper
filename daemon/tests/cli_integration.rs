use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn doctor_command_succeeds() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.arg("doctor")
        .assert()
        .success()
        .stdout(contains("required: rustc=ok cargo=ok"));
}

#[test]
fn ui_open_returns_url_for_known_extension() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "ui",
        "open",
        "--extension",
        "desktop-torrent-organizer",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
        "--idle-timeout-ms",
        "20",
        "--no-browser",
    ])
    .assert()
    .success()
    .stdout(contains("Config UI available at http://127.0.0.1:"));
}

#[test]
fn ui_open_fails_for_unknown_extension() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "ui",
        "open",
        "--extension",
        "missing-ext",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
        "--idle-timeout-ms",
        "20",
        "--no-browser",
    ])
    .assert()
    .failure()
    .stderr(contains("extension 'missing-ext' not found"));
}

#[test]
fn verify_command_succeeds_for_sample_extensions() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "verify",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(contains("Verified "))
    .stdout(contains(" extension(s)"));
}

#[test]
fn trigger_returns_error_for_unknown_extension() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "trigger",
        "does-not-exist",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .failure()
    .stderr(contains("extension 'does-not-exist' not found"));
}

#[test]
fn list_shows_sample_extension() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "list",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(contains("sort-downloads"))
    .stdout(contains("desktop-torrent-organizer"));
}

#[test]
fn validate_accepts_sample_descriptor() {
    let descriptor = repo_root().join("extensions/sort-downloads/manifest.json");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args(["validate", descriptor.to_str().expect("utf-8 path")])
        .assert()
        .success()
        .stdout(contains("OK: Sort Downloads"));
}

#[test]
fn validate_accepts_desktop_torrent_descriptor() {
    let descriptor = repo_root().join("extensions/desktop-torrent-organizer/manifest.json");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args(["validate", descriptor.to_str().expect("utf-8 path")])
        .assert()
        .success()
        .stdout(contains("OK: Desktop Torrent Organizer"));
}

#[test]
fn trigger_shows_selected_action_details() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "trigger",
        "sort-downloads",
        "--action",
        "sort",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(contains("Trigger dry-run"))
    .stdout(contains("Permissions: fs,ui"));
}

#[test]
fn trigger_torrent_extension_shows_expected_permissions() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "trigger",
        "desktop-torrent-organizer",
        "--action",
        "move-torrents",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(contains("Trigger dry-run"))
    .stdout(contains("Permissions: fs,shell,store,ui"))
    .stdout(contains("Move .torrent files"));
}

#[test]
fn trigger_session_counter_prints_current_value() {
    let extensions_dir = repo_root().join("extensions");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "trigger",
        "session-counter",
        "--action",
        "increment",
        "--extensions-dir",
        extensions_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(contains("Session counter:"));
}

#[test]
fn generate_main_writes_output_file() {
    let temp = tempdir().expect("tempdir");
    let descriptor = temp.path().join("manifest.json");
    let output = temp.path().join("generated-main.ts");
    fs::write(
        &descriptor,
        r#"{
            "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
            "id": "tmp-ext",
            "name": "Tmp Extension",
            "version": "1.0.0",
            "trigger": "tmp",
            "actions": [
                { "id": "run", "label": "Run", "script": "const value = 42;" }
            ]
        }"#,
    )
    .expect("write descriptor");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "generate-main",
        descriptor.to_str().expect("utf-8 path"),
        "--output",
        output.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success();

    let generated = fs::read_to_string(output).expect("generated file");
    assert!(generated.contains("const value = 42;"));
    assert!(generated.contains("api.notify"));
}

#[test]
fn verify_fails_for_extension_missing_main() {
    let temp = tempdir().expect("tempdir");
    let ext = temp.path().join("broken-ext");
    fs::create_dir_all(&ext).expect("create extension dir");
    fs::write(
        ext.join("manifest.json"),
        r#"{
            "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
            "id": "broken-ext",
            "name": "Broken Ext",
            "version": "1.0.0",
            "trigger": "broken",
            "actions": [
                { "id": "run", "label": "Run", "script": "return;" }
            ]
        }"#,
    )
    .expect("write descriptor");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("copperd"));
    cmd.args([
        "verify",
        "--extensions-dir",
        temp.path().to_str().expect("utf-8 path"),
    ])
    .assert()
    .failure()
    .stderr(contains("does not contain main.ts"));
}
