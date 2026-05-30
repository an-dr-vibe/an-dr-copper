mod tests {
    use super::{
        copper_data_root_from_home, execute_windows_display_action_with_runner_in, expand_home,
        extension_config_path_in, extension_status_path_in, handle_request,
        load_torrent_monitor_config, load_torrent_monitor_config_from,
        maybe_increment_session_counter, maybe_increment_session_counter_in,
        next_available_destination, parse_http_response, read_json_object, request_url,
        run_torrent_move, send_request, split_name_and_extension, write_desktop_torrent_status_in,
        write_json_object, DaemonConfig, DaemonState, IpcRequest, TorrentMonitorConfig,
        DEFAULT_BIND_ADDR, DEFAULT_RELOAD_INTERVAL_MS, WINDOWS_DISPLAY_MANAGER_ID,
    };
    use crate::daemon_service::DaemonControlService;
    use std::fs;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    use tempfile::tempdir;
    use tiny_http::{Response as HttpResponse, Server};

    fn write_extension(root: &Path, id: &str) {
        write_extension_with_action(root, id, "run");
    }

    fn write_extension_with_action(root: &Path, id: &str, action_id: &str) {
        let ext = root.join(id);
        fs::create_dir_all(&ext).expect("create extension directory");
        fs::write(
            ext.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "Test Extension",
                    "version": "1.0.0",
                    "trigger": "test",
                    "permissions": ["fs", "ui"],
                    "actions": [
                        {{ "id": "{action_id}", "label": "Run", "script": "return;" }}
                    ]
                }}"#
            ),
        )
        .expect("write descriptor");
        fs::write(
            ext.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");
    }

    fn write_windows_display_extension(root: &Path) {
        let ext = root.join("windows-display-manager");
        fs::create_dir_all(&ext).expect("create extension directory");
        fs::write(
            ext.join("manifest.json"),
            r#"{
                "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                "id": "windows-display-manager",
                "name": "Windows Display Manager",
                "version": "1.0.0",
                "trigger": "windows-display",
                "permissions": ["ui", "store"],
                "actions": [
                    { "id": "status", "label": "Status", "script": "status" },
                    { "id": "toggle-taskbar-autohide", "label": "Toggle", "script": "toggle" }
                ]
            }"#,
        )
        .expect("write descriptor");
        fs::write(
            ext.join("main.ts"),
            "export default function(){ return {}; }",
        )
        .expect("write main.ts");
    }

    #[test]
    fn daemon_config_default_matches_public_constants() {
        let config = DaemonConfig::default();
        assert_eq!(config.bind_addr, DEFAULT_BIND_ADDR);
        assert_eq!(
            config.reload_interval,
            Duration::from_millis(DEFAULT_RELOAD_INTERVAL_MS)
        );
        assert!(!config.extensions_dir.as_os_str().is_empty());
    }

    #[test]
    fn health_request_returns_extension_count() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Health, &running);
        assert!(response.ok);
        let data = response.data.expect("data");
        let count = data
            .get("extensionsLoaded")
            .and_then(|v| v.as_u64())
            .expect("count");
        assert!(
            count >= 1,
            "health should report at least the temp extension plus any shipped core extensions"
        );
        assert!(
            data.get("stateWarnings")
                .and_then(|value| value.as_array())
                .is_some(),
            "health should surface state diagnostics"
        );
    }

    #[test]
    fn trigger_request_returns_error_for_missing_extension() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "missing".to_string(),
                action: None,
            },
            &running,
        );
        assert!(!response.ok);
        assert!(response.message.contains("not found"));
    }

    #[test]
    fn reload_request_picks_new_extension() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let baseline_count = state.registry.list().count() as u64;

        write_extension(temp.path(), "new-ext");
        let response = handle_request(&mut state, IpcRequest::Reload, &running);
        assert!(response.ok);
        let count = response
            .data
            .expect("data")
            .get("extensionsLoaded")
            .and_then(|v| v.as_u64())
            .expect("count");
        assert_eq!(count, baseline_count + 1);
    }

    #[test]
    fn trigger_session_counter_includes_incremented_count() {
        let temp = tempdir().expect("tempdir");
        let status_home = temp.path().join("home");
        let data_root = copper_data_root_from_home(&status_home);
        let count1 = maybe_increment_session_counter_in(&data_root, "session-counter", "increment")
            .expect("increment")
            .expect("count");
        let count2 = maybe_increment_session_counter_in(&data_root, "session-counter", "increment")
            .expect("increment again")
            .expect("count again");
        let skipped = maybe_increment_session_counter_in(&data_root, "session-counter", "other")
            .expect("skip");
        assert_eq!(count1, 1);
        assert_eq!(count2, 2);
        assert!(skipped.is_none());
    }

    #[test]
    fn load_torrent_monitor_config_reads_polling_fields() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let ext_dir = data_root.join("desktop-torrent-organizer");
        fs::create_dir_all(&ext_dir).expect("create extension data dir");
        fs::write(
            ext_dir.join("data.json"),
            r#"{
              "autoRun": false,
              "pollIntervalSeconds": 12,
              "desktopFolder": "/tmp/desktop",
              "torrentsFolder": "/tmp/desktop/Torrents"
            }"#,
        )
        .expect("write config");

        let cfg = load_torrent_monitor_config_from(&data_root).expect("load");
        assert!(!cfg.enabled);
        assert_eq!(cfg.poll_interval.as_secs(), 12);
        assert_eq!(cfg.desktop_folder, PathBuf::from("/tmp/desktop"));
        assert_eq!(cfg.torrents_folder, PathBuf::from("/tmp/desktop/Torrents"));
    }

    #[test]
    fn run_torrent_move_moves_only_torrent_files() {
        let temp = tempdir().expect("tempdir");
        let desktop = temp.path().join("Desktop");
        let torrents = desktop.join("Torrents");
        fs::create_dir_all(&desktop).expect("create desktop");
        fs::write(desktop.join("movie.torrent"), "data").expect("write torrent");
        fs::write(desktop.join("note.txt"), "data").expect("write non-torrent");

        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: desktop.clone(),
            torrents_folder: torrents.clone(),
        };
        let report = run_torrent_move(&cfg).expect("run move");
        assert_eq!(report.found, 1);
        assert_eq!(report.moved, 1);
        assert_eq!(report.failed, 0);
        assert!(torrents.join("movie.torrent").exists());
        assert!(desktop.join("note.txt").exists());
    }

    #[test]
    fn list_request_returns_permissions_as_strings() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::List, &running);
        assert!(response.ok);
        let list = response
            .data
            .expect("data")
            .as_array()
            .expect("array")
            .to_vec();
        assert!(!list.is_empty());
        let permissions = list[0]
            .get("permissions")
            .and_then(|v| v.as_array())
            .expect("permissions");
        assert!(permissions.iter().any(|value| value.as_str() == Some("fs")));
    }

    #[test]
    fn shutdown_request_flips_running_flag() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Shutdown, &running);
        assert!(response.ok);
        assert!(!running.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn split_name_and_extension_handles_edge_cases() {
        assert_eq!(
            split_name_and_extension("movie.torrent"),
            ("movie", "torrent")
        );
        assert_eq!(split_name_and_extension("archive"), ("archive", ""));
        assert_eq!(split_name_and_extension(".hidden"), (".hidden", ""));
    }

    #[test]
    fn next_available_destination_uses_suffix_on_collision() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("movie.torrent"), "existing").expect("write existing");
        fs::write(target.join("movie-1.torrent"), "existing").expect("write existing suffix");

        let candidate = next_available_destination(target, "movie.torrent".into());
        assert_eq!(
            candidate.file_name().and_then(|v| v.to_str()),
            Some("movie-2.torrent")
        );
    }

    #[test]
    fn json_object_helpers_roundtrip_and_sanitize() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("status.json");

        write_json_object(&path, &serde_json::json!({"ok":true})).expect("write object");
        let object = read_json_object(&path).expect("read object");
        assert_eq!(object.get("ok").and_then(|v| v.as_bool()), Some(true));

        fs::write(&path, "[]").expect("write non-object");
        assert_eq!(
            read_json_object(&path).expect("read non-object"),
            serde_json::json!({})
        );

        fs::write(&path, "{bad-json").expect("write invalid");
        assert_eq!(
            read_json_object(&path).expect("read invalid"),
            serde_json::json!({})
        );
    }

    #[test]
    fn write_json_object_creates_missing_parent_directories() {
        let temp = tempdir().expect("tempdir");
        let nested = temp.path().join("x").join("y").join("data.json");
        write_json_object(&nested, &serde_json::json!({"ok": true})).expect("write nested");
        assert!(nested.exists());
    }

    #[test]
    fn extension_storage_paths_are_scoped_to_extension() {
        let root = PathBuf::from("C:/tmp/.Copper/extensions");
        let config_path = extension_config_path_in(&root, "desktop-torrent-organizer");
        let status_path = extension_status_path_in(&root, "desktop-torrent-organizer");
        assert_eq!(
            config_path,
            root.join("desktop-torrent-organizer").join("config.json")
        );
        assert_eq!(
            status_path,
            root.join("desktop-torrent-organizer").join("status.json")
        );
    }

    #[test]
    fn verify_request_reports_verified_count() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(&mut state, IpcRequest::Verify, &running);
        assert!(response.ok);
        assert!(response.message.contains("verified"));
    }

    #[test]
    fn trigger_request_errors_for_unknown_action() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);

        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "alpha-ext".to_string(),
                action: Some("missing-action".to_string()),
            },
            &running,
        );
        assert!(!response.ok);
        assert!(response.message.contains("not found"));
    }

    #[test]
    fn run_torrent_move_handles_missing_desktop_folder() {
        let temp = tempdir().expect("tempdir");
        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: temp.path().join("does-not-exist"),
            torrents_folder: temp.path().join("Torrents"),
        };

        let report = run_torrent_move(&cfg).expect("missing folder should not fail");
        assert_eq!(report.found, 0);
        assert_eq!(report.moved, 0);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn run_torrent_move_errors_when_desktop_is_not_directory() {
        let temp = tempdir().expect("tempdir");
        let desktop_file = temp.path().join("Desktop");
        fs::write(&desktop_file, "not a dir").expect("write desktop file");
        let cfg = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(1),
            desktop_folder: desktop_file,
            torrents_folder: temp.path().join("Torrents"),
        };
        let err = run_torrent_move(&cfg).expect_err("must fail for non-directory desktop");
        assert!(
            err.kind() == std::io::ErrorKind::NotADirectory
                || err.kind() == std::io::ErrorKind::Other
        );
    }

    #[test]
    fn next_available_destination_handles_names_without_extension() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("README"), "existing").expect("write existing");

        let candidate = next_available_destination(target, "README".into());
        assert_eq!(
            candidate.file_name().and_then(|v| v.to_str()),
            Some("README-1")
        );
    }

    #[test]
    fn next_available_destination_uses_timestamp_fallback_after_many_collisions() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path();
        fs::write(target.join("movie.torrent"), "existing").expect("seed");
        for idx in 1..=9999u32 {
            fs::write(target.join(format!("movie-{idx}.torrent")), "existing")
                .expect("seed suffix");
        }

        let candidate = next_available_destination(target, "movie.torrent".into());
        let name = candidate
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name")
            .to_string();
        assert!(name.starts_with("movie-"));
        assert!(name.ends_with(".torrent"));
        assert!(!target.join(&name).exists());
    }

    #[test]
    fn write_desktop_torrent_status_persists_scan_fields() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let config = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            desktop_folder: temp.path().join("Desktop"),
            torrents_folder: temp.path().join("Desktop/Torrents"),
        };
        let report = super::TorrentMoveReport {
            found: 3,
            moved: 2,
            failed: 1,
        };

        write_desktop_torrent_status_in(&data_root, &config, report).expect("write status");
        let stored = read_json_object(&extension_status_path_in(
            &data_root,
            "desktop-torrent-organizer",
        ))
        .expect("read status");
        assert_eq!(stored.get("autoRun").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            stored.get("pollIntervalSeconds").and_then(|v| v.as_u64()),
            Some(5)
        );
        assert_eq!(
            stored.get("lastScanFound").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(
            stored.get("lastScanMoved").and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(
            stored.get("lastScanFailed").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert!(stored
            .get("lastMoveUnix")
            .and_then(|v| v.as_u64())
            .is_some());
    }

    fn free_addr() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free");
        let addr = listener.local_addr().expect("local addr");
        format!("127.0.0.1:{}", addr.port())
    }

    #[test]
    fn request_url_maps_health_to_http_endpoint() {
        assert_eq!(
            request_url("127.0.0.1:4765", &IpcRequest::Health),
            "http://127.0.0.1:4765/health"
        );
    }

    #[test]
    fn parse_http_response_rejects_empty_body() {
        let err = parse_http_response(200, String::new()).expect_err("must fail");
        assert!(err.to_string().contains("empty HTTP response"));
    }

    #[test]
    fn send_request_parses_successful_json_response() {
        let addr = free_addr();
        let server = Server::http(&addr).expect("bind");
        let join = std::thread::spawn(move || {
            let request = server.recv().expect("request");
            assert_eq!(request.url(), "/health");
            request
                .respond(HttpResponse::from_string(
                    r#"{"ok":true,"message":"healthy","data":{"x":1}}"#,
                ))
                .expect("respond");
        });

        let response = send_request(&addr, &IpcRequest::Health).expect("response");
        assert!(response.ok);
        assert_eq!(response.message, "healthy");
        join.join().expect("join");
    }

    #[test]
    fn trigger_request_without_action_uses_first_action() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "alpha-ext".to_string(),
                action: None,
            },
            &running,
        );
        assert!(response.ok);
        let data = response.data.expect("payload");
        assert_eq!(data.get("actionId").and_then(|v| v.as_str()), Some("run"));
    }

    #[test]
    fn trigger_request_session_counter_includes_count_payload() {
        let temp = tempdir().expect("tempdir");
        write_extension_with_action(temp.path(), "session-counter", "increment");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let running = AtomicBool::new(true);
        let response = handle_request(
            &mut state,
            IpcRequest::Trigger {
                id: "session-counter".to_string(),
                action: Some("increment".to_string()),
            },
            &running,
        );
        assert!(response.ok);
        let data = response.data.expect("payload");
        assert!(data.get("sessionCount").and_then(|v| v.as_u64()).is_some());
    }

    #[test]
    fn trigger_payload_windows_display_status_reports_host_execution() {
        let temp = tempdir().expect("tempdir");
        write_windows_display_extension(temp.path());
        let state = DaemonState::load(temp.path()).expect("state");
        let service = DaemonControlService::new(
            &state.user_extensions_dir,
            state.core_extensions_dir.as_deref(),
            &state.registry,
            &state.host_extensions,
            &state.state_store,
        );

        let result = service.trigger_payload("windows-display-manager", Some("status"));
        if cfg!(target_os = "windows") {
            let payload = result.expect("payload");
            assert_eq!(
                payload.get("extensionId").and_then(|v| v.as_str()),
                Some("windows-display-manager")
            );
            assert_eq!(
                payload.get("actionId").and_then(|v| v.as_str()),
                Some("status")
            );
            assert!(
                payload.get("hostExecution").is_some(),
                "windows-display status action should include host execution payload"
            );
        } else {
            let err = result.expect_err("non-windows should not support display manager");
            assert!(err.contains("only supported on Windows"));
        }
    }

    #[test]
    fn windows_display_actions_apply_saved_config_instead_of_status_snapshot() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path();
        write_json_object(
            &extension_config_path_in(data_root, WINDOWS_DISPLAY_MANAGER_ID),
            &serde_json::json!({
                "taskbarAutoHide": true,
                "resolutionWidth": 2560,
                "resolutionHeight": 1440,
                "refreshRate": 144,
                "scalePercent": 150
            }),
        )
        .expect("write config");
        write_json_object(
            &extension_status_path_in(data_root, WINDOWS_DISPLAY_MANAGER_ID),
            &serde_json::json!({
                "taskbarAutoHide": false,
                "resolutionWidth": 1920,
                "resolutionHeight": 1080,
                "refreshRate": 60,
                "scalePercent": 100
            }),
        )
        .expect("write status");

        let execution = execute_windows_display_action_with_runner_in(
            data_root,
            "set-resolution",
            |action_id, config| {
                assert_eq!(action_id, "set-resolution");
                assert_eq!(
                    config.get("resolutionWidth").and_then(|v| v.as_i64()),
                    Some(2560)
                );
                assert_eq!(
                    config.get("resolutionHeight").and_then(|v| v.as_i64()),
                    Some(1440)
                );
                assert_eq!(
                    config.get("refreshRate").and_then(|v| v.as_i64()),
                    Some(144)
                );
                Ok(serde_json::json!({
                    "ok": true,
                    "action": "set-resolution",
                    "applied": true,
                    "resolution": {
                        "width": 2560,
                        "height": 1440,
                        "refreshRate": 144
                    }
                }))
            },
        )
        .expect("execution");

        assert_eq!(
            execution.get("action").and_then(|value| value.as_str()),
            Some("set-resolution")
        );

        let status = read_json_object(&extension_status_path_in(
            data_root,
            WINDOWS_DISPLAY_MANAGER_ID,
        ))
        .expect("read status");
        assert_eq!(
            status.get("lastActionOk").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            status
                .get("resolutionWidth")
                .and_then(|value| value.as_i64()),
            Some(2560)
        );
        assert_eq!(
            status
                .get("resolutionHeight")
                .and_then(|value| value.as_i64()),
            Some(1440)
        );
        assert_eq!(
            status.get("refreshRate").and_then(|value| value.as_i64()),
            Some(144)
        );
    }

    #[test]
    fn verify_request_errors_when_loaded_extension_loses_main_file() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let mut state = DaemonState::load(temp.path()).expect("state");
        fs::remove_file(temp.path().join("alpha-ext").join("main.ts")).expect("remove main");
        let running = AtomicBool::new(true);
        let response = handle_request(&mut state, IpcRequest::Verify, &running);
        assert!(!response.ok);
        assert!(response.message.contains("missing main.ts"));
    }

    #[test]
    fn reload_request_reports_error_for_invalid_manifest() {
        let temp = tempdir().expect("tempdir");
        let mut state = DaemonState::load(temp.path()).expect("state");
        let broken = temp.path().join("broken-ext");
        fs::create_dir_all(&broken).expect("create broken dir");
        fs::write(broken.join("manifest.json"), "{}").expect("write invalid manifest");
        fs::write(broken.join("main.ts"), "export default function(){}").expect("write main");
        let running = AtomicBool::new(true);
        let response = handle_request(&mut state, IpcRequest::Reload, &running);
        assert!(!response.ok);
        assert!(response.message.contains("manifest"));
    }

    #[test]
    fn maybe_increment_session_counter_handles_missing_or_present_home() {
        match maybe_increment_session_counter("not-session-counter", "noop") {
            Ok(value) => assert!(value.is_none()),
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn load_torrent_monitor_config_handles_missing_or_present_home() {
        match load_torrent_monitor_config() {
            Ok(cfg) => {
                let secs = cfg.poll_interval.as_secs();
                assert!((1..=3600).contains(&secs));
            }
            Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn expand_home_handles_tilde_variants() {
        let expanded_home = expand_home("~");
        let expanded_child = expand_home("~/Desktop");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded_home, home);
            assert_eq!(expanded_child, home.join("Desktop"));
        } else {
            assert_eq!(expanded_home, PathBuf::from("~"));
            assert_eq!(expanded_child, PathBuf::from("~/Desktop"));
        }

        let literal = expand_home("C:/tmp/Desktop");
        assert_eq!(literal, PathBuf::from("C:/tmp/Desktop"));
    }

    #[test]
    fn write_desktop_torrent_status_skips_last_move_when_nothing_moved() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let config = TorrentMonitorConfig {
            enabled: true,
            poll_interval: Duration::from_secs(5),
            desktop_folder: temp.path().join("Desktop"),
            torrents_folder: temp.path().join("Desktop/Torrents"),
        };
        let report = super::TorrentMoveReport {
            found: 1,
            moved: 0,
            failed: 1,
        };

        write_desktop_torrent_status_in(&data_root, &config, report).expect("write status");
        let stored = read_json_object(&extension_status_path_in(
            &data_root,
            "desktop-torrent-organizer",
        ))
        .expect("read status");
        assert!(stored.get("lastMoveUnix").is_none());
    }

    #[test]
    fn load_torrent_monitor_config_prefers_config_file_and_falls_back_to_legacy_data_file() {
        let temp = tempdir().expect("tempdir");
        let data_root = temp.path().join(".Copper/extensions");
        let ext_dir = data_root.join("desktop-torrent-organizer");
        fs::create_dir_all(&ext_dir).expect("create extension dir");

        fs::write(
            ext_dir.join("data.json"),
            r#"{
                "desktopFolder": "D:/LegacyDesktop",
                "torrentsFolder": "D:/LegacyDesktop/Torrents",
                "autoRun": false,
                "pollIntervalSeconds": 33
            }"#,
        )
        .expect("write legacy data");

        let legacy = load_torrent_monitor_config_from(&data_root).expect("load legacy");
        assert_eq!(legacy.desktop_folder, PathBuf::from("D:/LegacyDesktop"));
        assert_eq!(
            legacy.torrents_folder,
            PathBuf::from("D:/LegacyDesktop/Torrents")
        );
        assert!(!legacy.enabled);
        assert_eq!(legacy.poll_interval.as_secs(), 33);

        fs::write(
            ext_dir.join("config.json"),
            r#"{
                "desktopFolder": "D:/ConfigDesktop",
                "torrentsFolder": "D:/ConfigDesktop/Torrents",
                "autoRun": true,
                "pollIntervalSeconds": 9
            }"#,
        )
        .expect("write config");

        let config = load_torrent_monitor_config_from(&data_root).expect("load config");
        assert_eq!(config.desktop_folder, PathBuf::from("D:/ConfigDesktop"));
        assert_eq!(
            config.torrents_folder,
            PathBuf::from("D:/ConfigDesktop/Torrents")
        );
        assert!(config.enabled);
        assert_eq!(config.poll_interval.as_secs(), 9);
    }
}
