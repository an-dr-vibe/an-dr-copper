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
    let normalized = raw.replace("\r\n", "\n");
    normalized
        .split("\n#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or(&normalized)
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

#[test]
fn release_tree_has_no_legacy_typescript_runtime() {
    let root = repo_root();
    let removed_paths = [
        "daemon/src/execution.rs",
        "daemon/src/runtime",
        "sdk/api.d.ts",
        "sdk/bridge.ts",
        "sdk/package.json",
    ];
    for relative in removed_paths {
        assert!(
            !root.join(relative).exists(),
            "legacy runtime path must be removed at cutover: {relative}"
        );
    }

    let cli = production_source("cli.rs");
    for removed_command in [
        "GenerateMain",
        "RuntimeTrigger",
        "generate-main",
        "deno",
        "dry-run mode",
    ] {
        assert!(
            !cli.contains(removed_command),
            "legacy CLI surface must be removed at cutover: {removed_command}"
        );
    }

    for document in ["docs/ARCHITECTURE.md", "docs/DEVELOPMENT.md"] {
        let source =
            fs::read_to_string(root.join(document)).expect("read current architecture doc");
        for removed_runtime in [
            "daemon/src/runtime",
            "runtime adapter abstraction",
            "execution.rs",
            "subprocess execution adapter",
        ] {
            assert!(
                !source.contains(removed_runtime),
                "{document} must not describe the removed runtime: {removed_runtime}"
            );
        }
    }

    for script in [
        "scripts/bootstrap.ps1",
        "scripts/build-release.ps1",
        "scripts/package-extension.ps1",
    ] {
        let source = fs::read_to_string(root.join(script)).expect("read release script");
        for legacy_runtime in ["deno", "main.ts"] {
            assert!(
                !source.to_ascii_lowercase().contains(legacy_runtime),
                "{script} must not reference the legacy runtime: {legacy_runtime}"
            );
        }
    }
}

#[test]
fn release_and_install_scripts_agree_on_the_copper_bundle_contract() {
    let root = repo_root();
    let release =
        fs::read_to_string(root.join("scripts/build-release.ps1")).expect("release script");
    assert!(
        release.contains("\"schemas\""),
        "release bundle must include the versioned descriptor schemas"
    );

    let installer = fs::read_to_string(root.join("scripts/install.ps1")).expect("install script");
    for stale_binary in ["copperd.exe", "\"copperd\""] {
        assert!(
            !installer.contains(stale_binary),
            "installer must consume the copper(.exe) binary emitted by Cargo: {stale_binary}"
        );
    }

    let coverage = fs::read_to_string(root.join("scripts/coverage.ps1")).expect("coverage script");
    assert!(
        coverage.contains("$CoverageRustVersion = \"1.94.0\"")
            && coverage.contains("$Toolchain = \"$CoverageRustVersion-x86_64-pc-windows-msvc\""),
        "coverage must use the first Rust toolchain supported by this Bones/Wasmtime generation"
    );
}
