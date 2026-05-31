mod tests {
    use super::{
        execute_windows_display_action_with_runner_in, extension_config_path_in,
        extension_status_path_in, handle_request, parse_http_response, read_json_object,
        request_url, send_request, write_json_object, DaemonConfig, DaemonState, IpcRequest,
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
        let config_path = extension_config_path_in(&root, "alpha-ext");
        let status_path = extension_status_path_in(&root, "alpha-ext");
        assert_eq!(config_path, root.join("alpha-ext").join("config.json"));
        assert_eq!(status_path, root.join("alpha-ext").join("status.json"));
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
    fn trigger_request_session_counter_succeeds() {
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
        assert_eq!(
            data.get("actionId").and_then(|v| v.as_str()),
            Some("increment")
        );
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

}
