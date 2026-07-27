    use super::{
        build_ui_state, core_data_path_for, extension_config_path_for, extension_status_path_for,
        dispatch_bones_request, load_config, parse_json_object, parse_request, refresh_ui_state,
        render_html, start_daemon_ui_server, store_config, visible_descriptors, write_response,
        HttpMethod, HttpResponse, UiConfigError, UiOpenOptions, UiTransport,
        COPPER_SETTINGS_PROTOCOL_V1,
    };
    use crate::config_ui_http::read_chunked_body;
    use crate::config_ui_service::{build_core_info, build_extension_info};
    use crate::control_plane::{ControlPlaneAuth, UI_AUTH_HEADER};
    use crate::core_config::CoreConfig;
    use crate::descriptor::{
        Action, Descriptor, InputField, InputType, Platform, RuntimeDescriptor, RuntimeKind,
        SettingsDescriptor, SettingsSection, StatusDescriptor, StatusField, StatusFieldFormat,
        UiDescriptor, COMPONENT_ABI_V1,
    };
    use crate::host_extensions::HostExtensionRegistry;
    use crate::state_store::ExtensionStateStore;
    use std::collections::HashSet;
    use std::fs;
    use std::io::{BufReader, Cursor, Read};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn sample_descriptor() -> Descriptor {
        Descriptor {
            schema: Some(
                "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json".to_string(),
            ),
            id: "desktop-torrent-organizer".to_string(),
            name: "Desktop Torrent Organizer".to_string(),
            version: "1.0.0".to_string(),
            trigger: "desktop-torrents".to_string(),
            runtime: None,
            platforms: vec![],
            permissions: vec![],
            inputs: vec![InputField {
                id: "desktopFolder".to_string(),
                field_type: InputType::FolderPicker,
                label: "Desktop folder".to_string(),
                description: Some("Folder that Copper scans for incoming .torrent files.".to_string()),
                default: serde_json::json!("~/Desktop"),
                options: vec![],
                options_source: None,
            }],
            actions: vec![Action {
                id: "move-torrents".to_string(),
                label: "Move .torrent files".to_string(),
                description: Some("Run the organizer immediately using the saved settings.".to_string()),
                script: "Move .torrent files".to_string(),
            }],
            ui: Some(UiDescriptor {
                ui_type: "form".to_string(),
                source: None,
                on_select: None,
            }),
            settings: Some(SettingsDescriptor {
                title: Some("Desktop Torrent Organizer".to_string()),
                description: Some(
                    "Configure how Copper watches the desktop and review the latest monitor status."
                        .to_string(),
                ),
                apply_actions: vec![],
                tabs: vec![],
                sections: vec![SettingsSection {
                    id: "monitor".to_string(),
                    title: "Monitor".to_string(),
                    description: Some(
                        "Settings used by the background desktop torrent watcher.".to_string(),
                    ),
                    inputs: vec!["desktopFolder".to_string()],
                }],
                status: Some(StatusDescriptor {
                    title: Some("Current status".to_string()),
                    description: Some(
                        "Latest runtime values reported by the daemon.".to_string(),
                    ),
                    fields: vec![StatusField {
                        key: "lastScanUnix".to_string(),
                        label: "Last scan".to_string(),
                        format: Some(StatusFieldFormat::DateTime),
                    }],
                }),
            }),
            tray: None,
        }
    }

    #[test]
    fn bones_transport_is_versioned_correlated_and_uses_shared_settings_routes() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);
        let mut state = build_ui_state(
            temp.path(),
            Some("desktop-torrent-organizer"),
            true,
            ControlPlaneAuth::ephemeral(),
            "bones://settings".to_string(),
        )
        .expect("settings state");
        state.transport = UiTransport::Bones;

        let response: serde_json::Value = serde_json::from_str(&dispatch_bones_request(
            &state,
            r#"{
                "protocol":"copper.settings/1",
                "requestId":"settings-7",
                "method":"POST",
                "path":"/config/extension/desktop-torrent-organizer",
                "body":{"desktopFolder":"D:/Incoming"}
            }"#,
        ))
        .expect("response json");

        assert_eq!(response["protocol"], COPPER_SETTINGS_PROTOCOL_V1);
        assert_eq!(response["requestId"], "settings-7");
        assert_eq!(response["ok"], true);
        assert_eq!(response["status"], 200);
        assert_eq!(
            state
                .state_store
                .inspect_config("desktop-torrent-organizer")
                .expect("saved config")
                .value["desktopFolder"],
            "D:/Incoming"
        );

        let wrong_protocol: serde_json::Value = serde_json::from_str(&dispatch_bones_request(
            &state,
            r#"{
                "protocol":"copper.settings/2",
                "requestId":"settings-8",
                "method":"GET",
                "path":"/config/core"
            }"#,
        ))
        .expect("error json");
        assert_eq!(wrong_protocol["requestId"], "settings-8");
        assert_eq!(wrong_protocol["ok"], false);
        assert_eq!(wrong_protocol["status"], 400);
        assert!(wrong_protocol["error"]
            .as_str()
            .expect("error")
            .contains("unsupported settings protocol"));
    }

    #[test]
    fn bones_html_selects_ipc_transport_without_exposing_http_credentials() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), &sample_descriptor());
        let mut state = build_ui_state(
            temp.path(),
            None,
            true,
            ControlPlaneAuth::ephemeral(),
            "bones://settings".to_string(),
        )
        .expect("settings state");
        state.transport = UiTransport::Bones;

        let html = render_html(&state);

        assert!(html.contains(r#""transport":"bones""#));
        assert!(html.contains(COPPER_SETTINGS_PROTOCOL_V1));
        assert!(html.contains("window.ipc.postMessage"));
        assert!(!html.contains(state.auth_token.as_str()));
    }

    #[test]
    fn settings_catalog_refreshes_after_core_enablement_changes_without_reopening() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), &sample_descriptor());
        let mut state = build_ui_state(
            temp.path(),
            None,
            true,
            ControlPlaneAuth::ephemeral(),
            "bones://settings".to_string(),
        )
        .expect("settings state");
        state.state_store = ExtensionStateStore::new(temp.path().join("state"));
        refresh_ui_state(&mut state).expect("initial refresh");
        assert_eq!(state.descriptors.len(), 1);

        store_config(
            &state.state_store.core_config_path(),
            &serde_json::json!({
                "disabledExtensions": ["desktop-torrent-organizer"]
            }),
        )
        .expect("disable extension");
        refresh_ui_state(&mut state).expect("refresh after disable");

        assert!(state.descriptors.is_empty());
        assert!(state.extension_ids.is_empty());
        assert_eq!(state.discoverable_descriptors.len(), 1);
    }

    fn sample_state() -> super::UiServerState {
        let descriptor = sample_descriptor();
        super::UiServerState {
            selected_extension_id: descriptor.id.clone(),
            extension_ids: [descriptor.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![descriptor.clone()],
            discoverable_descriptors: vec![descriptor],
            core_extension_ids: ["desktop-torrent-organizer".to_string()]
                .into_iter()
                .collect::<HashSet<_>>(),
            user_extensions_dir: PathBuf::from("C:/tmp/copper-user"),
            core_extensions_dir: Some(PathBuf::from("C:/tmp/copper-core")),
            runtime_extension_roots: vec![
                PathBuf::from("C:/tmp/copper-core"),
                PathBuf::from("C:/tmp/copper-user"),
            ],
            state_store: ExtensionStateStore::new(PathBuf::from("C:/tmp/.Copper/extensions")),
            host_extensions: HostExtensionRegistry::new(),
            auth_token: "test-auth-token".to_string(),
            origin: "http://127.0.0.1:4766".to_string(),
            allow_close: true,
            transport: UiTransport::Http,
        }
    }

    fn test_origin() -> String {
        "http://127.0.0.1:4766".to_string()
    }

    fn test_auth() -> ControlPlaneAuth {
        ControlPlaneAuth::ephemeral()
    }

    #[test]
    fn ui_open_options_default_values_are_stable() {
        let options = UiOpenOptions::default();
        assert_eq!(options.bind_addr, "127.0.0.1:0");
        assert!(!options.open_browser);
        assert!(options.open_window);
        assert_eq!(options.idle_timeout, Duration::from_secs(300));
    }

    fn write_extension(root: &std::path::Path, descriptor: &Descriptor) {
        let ext = root.join(&descriptor.id);
        fs::create_dir_all(&ext).expect("create extension dir");
        let mut manifest = serde_json::json!({
            "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
            "id": descriptor.id,
            "name": descriptor.name,
            "version": descriptor.version,
            "trigger": descriptor.trigger,
            "runtime": {
                "kind": "wasm-component",
                "abi": "copper.component/1",
                "artifact": format!("{}.wasm", descriptor.id)
            },
            "permissions": [],
            "inputs": [{
                "id": "desktopFolder",
                "type": "folder-picker",
                "label": "Desktop folder",
                "default": "~/Desktop"
            }],
            "actions": [{
                "id": "move-torrents",
                "label": "Move .torrent files",
                "script": "return;"
            }],
            "ui": { "type": "form" }
        });
        if !descriptor.platforms.is_empty() {
            manifest["platforms"] = serde_json::Value::Array(
                descriptor
                    .platforms
                    .iter()
                    .map(|platform| serde_json::json!(platform.as_str()))
                    .collect::<Vec<_>>(),
            );
        }
        fs::write(
            ext.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).expect("descriptor json"),
        )
        .expect("write manifest");
        fs::write(ext.join(format!("{}.wasm", descriptor.id)), b"\0asm")
            .expect("write component");
    }

    fn parse_http_url(url: &str) -> String {
        url.strip_prefix("http://").expect("http url").to_string()
    }

    #[test]
    fn build_ui_state_rejects_unknown_selected_extension() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);
        let err = build_ui_state(
            temp.path(),
            Some("missing-extension"),
            true,
            test_auth(),
            test_origin(),
        )
        .expect_err("must fail");
        match err {
            UiConfigError::ExtensionNotFound(id) => assert_eq!(id, "missing-extension"),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn build_ui_state_defaults_to_core_section() {
        let temp = tempdir().expect("tempdir");
        let mut descriptor = sample_descriptor();
        descriptor.id = "alpha-ext".to_string();
        descriptor.name = "Alpha Extension".to_string();
        write_extension(temp.path(), &descriptor);
        let state = build_ui_state(temp.path(), None, true, test_auth(), test_origin())
            .expect("build state");
        assert!(state.selected_extension_id.is_empty());
    }

    #[test]
    fn build_ui_state_keeps_platform_restricted_extensions_visible() {
        let temp = tempdir().expect("tempdir");
        let mut descriptor = sample_descriptor();
        descriptor.id = "platform-bound".to_string();
        descriptor.name = "Platform Bound".to_string();
        descriptor.platforms = vec![match crate::extension::current_platform() {
            Platform::Windows => Platform::Linux,
            Platform::Macos => Platform::Windows,
            Platform::Linux => Platform::Windows,
        }];
        write_extension(temp.path(), &descriptor);

        let state = build_ui_state(
            temp.path(),
            Some("platform-bound"),
            true,
            test_auth(),
            test_origin(),
        )
        .expect("build");
        assert!(state.extension_ids.contains("platform-bound"));
    }

    #[test]
    fn visible_descriptors_hide_disabled_extensions_from_sidebar_only() {
        let enabled = sample_descriptor();
        let mut disabled = sample_descriptor();
        disabled.id = "disabled-ext".to_string();
        disabled.name = "Disabled Extension".to_string();

        let descriptors = vec![enabled.clone(), disabled.clone()];
        let core_config = CoreConfig {
            disabled_extensions: ["disabled-ext".to_string()].into_iter().collect(),
        };

        let visible = visible_descriptors(&descriptors, &core_config);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, enabled.id);
        assert_eq!(descriptors.len(), 2, "discoverable list stays intact");
    }

    #[test]
    fn find_discoverable_descriptor_reads_hidden_extension_metadata() {
        let enabled = sample_descriptor();
        let mut hidden = sample_descriptor();
        hidden.id = "hidden-ext".to_string();
        hidden.name = "Hidden Extension".to_string();

        let state = super::UiServerState {
            selected_extension_id: enabled.id.clone(),
            extension_ids: [enabled.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![enabled.clone()],
            discoverable_descriptors: vec![enabled, hidden.clone()],
            core_extension_ids: HashSet::new(),
            user_extensions_dir: PathBuf::from("C:/tmp/copper-user"),
            core_extensions_dir: Some(PathBuf::from("C:/tmp/copper-core")),
            runtime_extension_roots: vec![PathBuf::from("C:/tmp/copper-user")],
            state_store: ExtensionStateStore::new(PathBuf::from("C:/tmp/.Copper/extensions")),
            host_extensions: HostExtensionRegistry::new(),
            auth_token: "test-auth-token".to_string(),
            origin: "http://127.0.0.1:4766".to_string(),
            allow_close: true,
            transport: UiTransport::Http,
        };

        let descriptor =
            super::find_discoverable_descriptor(&state, "hidden-ext").expect("discoverable");
        assert_eq!(descriptor.id, hidden.id);
    }

    #[test]
    fn build_extension_info_supports_hidden_discoverable_extension_commands() {
        let enabled = sample_descriptor();
        let hidden = Descriptor {
            schema: Some(
                "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json".to_string(),
            ),
            id: "hidden-ext".to_string(),
            name: "Hidden Extension".to_string(),
            version: "1.0.0".to_string(),
            trigger: "hidden-trigger".to_string(),
            runtime: None,
            platforms: vec![],
            permissions: vec![],
            inputs: vec![],
            actions: vec![
                Action {
                    id: "status".to_string(),
                    label: "Read status".to_string(),
                    description: Some("Read current extension state.".to_string()),
                    script: "status".to_string(),
                },
                Action {
                    id: "apply-hidden".to_string(),
                    label: "Apply hidden config".to_string(),
                    description: Some("Apply the saved hidden extension config.".to_string()),
                    script: "apply-hidden".to_string(),
                },
            ],
            ui: Some(UiDescriptor {
                ui_type: "form".to_string(),
                source: None,
                on_select: None,
            }),
            settings: Some(SettingsDescriptor {
                title: Some("Hidden Extension".to_string()),
                description: Some("Hidden settings".to_string()),
                apply_actions: vec!["apply-hidden".to_string()],
                tabs: vec![],
                sections: vec![],
                status: None,
            }),
            tray: None,
        };

        let state = super::UiServerState {
            selected_extension_id: enabled.id.clone(),
            extension_ids: [enabled.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![enabled],
            discoverable_descriptors: vec![hidden.clone()],
            core_extension_ids: HashSet::new(),
            user_extensions_dir: PathBuf::from("C:/tmp/copper-user"),
            core_extensions_dir: Some(PathBuf::from("C:/tmp/copper-core")),
            runtime_extension_roots: vec![PathBuf::from("C:/tmp/copper-user")],
            state_store: ExtensionStateStore::new(PathBuf::from("C:/tmp/.Copper/extensions")),
            host_extensions: HostExtensionRegistry::new(),
            auth_token: "test-auth-token".to_string(),
            origin: "http://127.0.0.1:4766".to_string(),
            allow_close: true,
            transport: UiTransport::Http,
        };

        let info = build_extension_info(&state, &hidden).expect("hidden extension info");
        let commands = info
            .get("commands")
            .and_then(|value| value.as_array())
            .expect("commands array");
        assert!(
            !commands.is_empty(),
            "discoverable extensions should keep command metadata for the core page"
        );
    }

    fn http_request(addr: &str, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
        http_request_with_headers(addr, method, path, &[], body)
    }

    fn http_request_with_headers(
        addr: &str,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> (u16, String) {
        let payload = body.unwrap_or("");
        let mut stream = TcpStream::connect(addr).expect("connect");
        let header_block = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect::<String>();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n{header_block}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        std::io::Write::write_all(&mut stream, request.as_bytes()).expect("send request");
        std::io::Write::flush(&mut stream).expect("flush request");

        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read response");
        let mut lines = raw.lines();
        let status_line = lines.next().unwrap_or_default().to_string();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<u16>().ok())
            .expect("status code");
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    fn extract_auth_token(html: &str) -> String {
        html.split("\"authToken\":\"")
            .nth(1)
            .and_then(|tail| tail.split('"').next())
            .map(str::to_string)
            .expect("auth token in html")
    }

    #[test]
    fn render_html_contains_extension_pages_and_status_view() {
        let html = render_html(&sample_state());
        assert!(html.contains("Settings"));
        assert!(html.contains("Copper"));
        assert!(html.contains("Core Extensions"));
        assert!(html.contains("Other Extensions"));
        assert!(html.contains("Launch Copper at login"));
        assert!(html.contains("UI theme"));
        assert!(html.contains("Desktop Torrent Organizer"));
        assert!(html.contains("Save settings"));
        assert!(html.contains("Status"));
        assert!(html.contains("renderCommandsPage"));
        assert!(html.contains("Recent status"));
        assert!(html.contains("URLSearchParams(window.location.search)"));
        assert!(html.contains("const THEME_OPTIONS"));
        assert!(html.contains("id: 'light', label: 'Light'"));
        assert!(html.contains("id: 'dark', label: 'Dark'"));
        assert!(!html.contains("Copper Dark"));
        assert!(!html.contains("Brass Dark"));
        assert!(!html.contains("Silver Dark"));
        assert!(!html.contains("Gold Dark"));
        assert!(!html.contains("Titanium Dark"));
        assert!(html.contains("applyTheme"));
        assert!(html.contains("unsaved-badge"));
        assert!(html.contains("refreshDirtyState"));
        assert!(html.contains("</style>"));
        assert_eq!(html.matches("<script>").count(), 1);
        assert!(html.contains(UI_AUTH_HEADER));
        assert!(!html.contains("{UI_AUTH_HEADER}"));
    }

    #[test]
    fn render_html_applies_saved_core_theme_before_first_section_load() {
        let temp = tempdir().expect("tempdir");
        let mut state = sample_state();
        state.state_store = ExtensionStateStore::new(temp.path().join(".Copper/extensions"));
        let core_path = state.state_store.core_config_path();
        fs::create_dir_all(core_path.parent().expect("parent")).expect("create parent");
        fs::write(&core_path, r#"{"uiTheme":"obsidian-dark"}"#).expect("write core config");

        let html = render_html(&state);

        assert!(html.contains(r#""coreUiTheme":"obsidian-dark""#));
        assert!(html.contains("applyTheme(model.coreUiTheme || 'light');"));
        assert!(html.contains("if (normalized === 'obsidian-dark') return 'dark';"));
    }

    #[test]
    fn config_and_status_paths_are_separated() {
        let root = PathBuf::from("C:/tmp/copper-user");
        assert_eq!(
            core_data_path_for(&root),
            root.join("copper-core").join("config.json")
        );
        assert_eq!(
            extension_config_path_for(&root, "desktop-torrent-organizer"),
            root.join("desktop-torrent-organizer").join("config.json")
        );
        assert_eq!(
            extension_status_path_for(&root, "desktop-torrent-organizer"),
            root.join("desktop-torrent-organizer").join("status.json")
        );
    }

    #[test]
    fn core_info_includes_runtime_roots() {
        let state = sample_state();
        let info = build_core_info(&state).expect("core info");
        let roots = info
            .get("runtimeExtensionRoots")
            .and_then(|v| v.as_array())
            .expect("roots array");
        assert_eq!(roots.len(), 2);
        assert!(
            info.get("dataRoot").is_some(),
            "core info should include extension data root"
        );
        assert_eq!(
            info.get("hostPlatform").and_then(|v| v.as_str()),
            Some(crate::extension::current_platform().as_str())
        );
        assert!(
            info.get("stateWarnings")
                .and_then(|value| value.as_array())
                .is_some(),
            "core info should expose state diagnostics"
        );
        assert!(
            info.get("selectedExtensionId").is_none(),
            "core status should not expose the currently selected extension"
        );
    }

    #[test]
    fn build_extension_info_exposes_all_component_actions_through_the_local_cli() {
        let state = sample_state();
        let descriptor = state.descriptors[0].clone();
        let info = super::build_extension_info(&state, &descriptor).expect("info");
        assert!(
            info.get("stateWarnings")
                .and_then(|value| value.as_array())
                .is_some(),
            "extension info should expose state diagnostics"
        );
        let commands = info
            .get("commands")
            .and_then(|value| value.as_array())
            .expect("commands array");
        assert_eq!(commands.len(), 1);
        assert!(commands[0]["usage"][0]
            .as_str()
            .unwrap_or_default()
            .contains("copperd trigger desktop-torrent-organizer --action move-torrents"));
    }

    #[test]
    fn build_extension_info_exposes_component_actions_through_the_local_cli() {
        let mut state = sample_state();
        state.descriptors[0].runtime = Some(RuntimeDescriptor {
            kind: RuntimeKind::WasmComponent,
            abi: COMPONENT_ABI_V1.to_string(),
            artifact: "desktop-torrent-organizer.wasm".to_string(),
            background: None,
        });
        let descriptor = state.descriptors[0].clone();
        let info = super::build_extension_info(&state, &descriptor).expect("info");
        let commands = info["commands"].as_array().expect("commands");
        assert_eq!(commands.len(), 1);
        assert!(commands[0]["usage"][0]
            .as_str()
            .unwrap_or_default()
            .contains("copperd trigger desktop-torrent-organizer --action move-torrents"));
    }

    #[test]
    fn build_extension_info_explains_user_accessible_commands() {
        let descriptor = Descriptor {
            schema: Some(
                "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json".to_string(),
            ),
            id: "windows-display-manager".to_string(),
            name: "Windows Display Manager".to_string(),
            version: "1.0.0".to_string(),
            trigger: "windows-display".to_string(),
            runtime: None,
            platforms: vec![],
            permissions: vec![],
            inputs: vec![],
            actions: vec![
                Action {
                    id: "status".to_string(),
                    label: "Read status".to_string(),
                    description: Some("Read current display state.".to_string()),
                    script: "status".to_string(),
                },
                Action {
                    id: "set-resolution".to_string(),
                    label: "Set resolution".to_string(),
                    description: Some("Apply the saved resolution.".to_string()),
                    script: "set-resolution".to_string(),
                },
            ],
            ui: Some(UiDescriptor {
                ui_type: "form".to_string(),
                source: None,
                on_select: None,
            }),
            settings: Some(SettingsDescriptor {
                title: Some("Display".to_string()),
                description: Some("Configure Windows display settings.".to_string()),
                apply_actions: vec!["set-resolution".to_string()],
                tabs: vec![],
                sections: vec![],
                status: None,
            }),
            tray: None,
        };
        let state = super::UiServerState {
            selected_extension_id: descriptor.id.clone(),
            extension_ids: [descriptor.id.clone()].into_iter().collect::<HashSet<_>>(),
            descriptors: vec![descriptor.clone()],
            discoverable_descriptors: vec![descriptor.clone()],
            core_extension_ids: HashSet::new(),
            user_extensions_dir: PathBuf::from("C:/tmp/copper-user"),
            core_extensions_dir: Some(PathBuf::from("C:/tmp/copper-core")),
            runtime_extension_roots: vec![PathBuf::from("C:/tmp/copper-user")],
            state_store: ExtensionStateStore::new(PathBuf::from("C:/tmp/.Copper/extensions")),
            host_extensions: HostExtensionRegistry::new(),
            auth_token: "test-auth-token".to_string(),
            origin: "http://127.0.0.1:4766".to_string(),
            allow_close: true,
            transport: UiTransport::Http,
        };

        let info = super::build_extension_info(&state, &descriptor).expect("info");
        let commands = info
            .get("commands")
            .and_then(|value| value.as_array())
            .expect("commands array");
        assert_eq!(commands.len(), 2);

        let status_command = commands
            .iter()
            .find(|value| value.get("id").and_then(|v| v.as_str()) == Some("status"))
            .expect("status command");
        let status_usage = status_command
            .get("usage")
            .and_then(|value| value.as_array())
            .expect("status usage");
        assert_eq!(status_usage.len(), 1);
        assert!(status_usage[0]
            .as_str()
            .unwrap_or_default()
            .contains("copperd trigger windows-display-manager --action status"));

        let set_resolution_command = commands
            .iter()
            .find(|value| value.get("id").and_then(|v| v.as_str()) == Some("set-resolution"))
            .expect("set-resolution command");
        let set_resolution_usage = set_resolution_command
            .get("usage")
            .and_then(|value| value.as_array())
            .expect("set-resolution usage");
        assert_eq!(set_resolution_usage.len(), 2);
        assert!(set_resolution_usage.iter().any(|value| value.as_str()
            == Some("Update the saved settings above, then click Save and apply.")));
    }

    #[test]
    fn render_html_hides_close_button_when_close_disabled() {
        let mut state = sample_state();
        state.allow_close = false;
        let html = render_html(&state);
        assert!(html.contains("if (!model.allowClose && closeBtn)"));
    }

    #[test]
    fn reads_chunked_body() {
        let raw = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let body = read_chunked_body(&mut reader).expect("chunked body");
        assert_eq!(body, b"Wikipedia");
    }

    #[test]
    fn rejects_invalid_chunk_size() {
        let raw = b"ZZ\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("invalid chunk size");
        assert!(err.to_string().contains("invalid chunk size"));
    }

    #[test]
    fn store_config_merges_and_removes_keys_without_touching_status() {
        let temp = tempdir().expect("tempdir");
        let path = temp
            .path()
            .join("desktop-torrent-organizer")
            .join("config.json");
        let status_path = temp
            .path()
            .join("desktop-torrent-organizer")
            .join("status.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(
            &path,
            r#"{
              "desktopFolder":"~/Desktop",
              "obsoleteAction":"move-torrents"
            }"#,
        )
        .expect("seed data");
        fs::write(&status_path, r#"{ "lastScanUnix": 1 }"#).expect("seed status");

        let update = serde_json::json!({
            "desktopFolder": "D:/Desktop",
            "__remove": ["obsoleteAction"]
        });
        store_config(&path, &update).expect("store config");

        let stored = load_config(&path).expect("load");
        assert_eq!(
            stored.get("desktopFolder").and_then(|v| v.as_str()),
            Some("D:/Desktop")
        );
        assert!(stored.get("obsoleteAction").is_none());

        let status = load_config(&status_path).expect("load status");
        assert_eq!(status.get("lastScanUnix").and_then(|v| v.as_u64()), Some(1));
    }

    fn parse_over_loopback(raw_request: &'static str) -> super::HttpRequest {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, raw_request.as_bytes()).expect("write request");
        });

        let (stream, _) = listener.accept().expect("accept");
        let parsed = parse_request(stream).expect("parse request");
        sender.join().expect("join sender");
        parsed
    }

    #[test]
    fn parse_json_object_validates_top_level_type() {
        let err = parse_json_object(br#"["not","object"]"#).expect_err("must reject non-object");
        assert!(err.to_string().contains("JSON object"));

        let ok = parse_json_object(br#"{"enabled":true}"#).expect("object");
        assert_eq!(ok.get("enabled").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn parse_request_reads_content_length_body() {
        let request = parse_over_loopback(
            "POST /config/core HTTP/1.1\r\nHost: localhost\r\nContent-Length: 12\r\n\r\n{\"k\":\"v123\"}",
        );
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/config/core");
        assert_eq!(request.body, br#"{"k":"v123"}"#);
    }

    #[test]
    fn parse_request_reads_chunked_payload() {
        let request = parse_over_loopback(
            "POST /config/core HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n",
        );
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/config/core");
        assert_eq!(request.body, b"Wikipedia");
    }

    #[test]
    fn parse_request_rejects_unsupported_method() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(
                &mut client,
                b"PUT /config/core HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("unsupported method");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("unsupported method"));
    }

    #[test]
    fn parse_request_rejects_empty_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let stream = TcpStream::connect(addr).expect("connect");
            drop(stream);
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("empty request");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("empty request"));
    }

    #[test]
    fn parse_request_rejects_missing_path() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, b"GET\r\n\r\n").expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("missing path");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("missing path"));
    }

    #[test]
    fn parse_request_rejects_missing_method() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let sender = thread::spawn(move || {
            let mut client = TcpStream::connect(addr).expect("connect");
            std::io::Write::write_all(&mut client, b"\r\n\r\n").expect("write");
        });

        let (stream, _) = listener.accept().expect("accept");
        let err = parse_request(stream).expect_err("missing method");
        sender.join().expect("join sender");
        assert!(err.to_string().contains("missing method"));
    }

    #[test]
    fn read_chunked_body_rejects_unexpected_eof() {
        let raw = b"";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("unexpected eof");
        assert!(err.to_string().contains("unexpected EOF"));
    }

    #[test]
    fn read_chunked_body_rejects_invalid_chunk_terminator() {
        let raw = b"1\r\naZZ0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let err = read_chunked_body(&mut reader).expect_err("invalid chunk terminator");
        assert!(err.to_string().contains("invalid chunk terminator"));
    }

    #[test]
    fn read_chunked_body_skips_trailers() {
        let raw = b"1\r\na\r\n0\r\nX-Test: 1\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let body = read_chunked_body(&mut reader).expect("chunked with trailer");
        assert_eq!(body, b"a");
    }

    #[test]
    fn load_config_sanitizes_invalid_and_non_object_payloads() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("bad.json");

        fs::write(&path, "[]").expect("write non-object");
        let non_object = load_config(&path).expect("read non-object");
        assert_eq!(non_object, serde_json::json!({}));

        fs::write(&path, "{this-is-not-json").expect("write invalid");
        let invalid = load_config(&path).expect("read invalid");
        assert_eq!(invalid, serde_json::json!({}));
    }

    #[test]
    fn load_config_returns_empty_for_missing_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("missing.json");
        let value = load_config(&path).expect("load missing");
        assert_eq!(value, serde_json::json!({}));
    }

    #[test]
    fn store_config_replaces_with_non_object_payload_and_creates_parent() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("nested").join("config.json");
        store_config(&path, &serde_json::json!("raw-string")).expect("store non-object");
        let raw = fs::read_to_string(&path).expect("read file");
        assert!(raw.contains("raw-string"));
    }

    #[test]
    fn write_response_uses_ok_text_for_unknown_status_code() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            let mut raw = String::new();
            stream.read_to_string(&mut raw).expect("read");
            raw
        });

        let (mut stream, _) = listener.accept().expect("accept");
        write_response(
            &mut stream,
            HttpResponse {
                status: 500,
                content_type: "text/plain; charset=utf-8",
                body: b"boom".to_vec(),
            },
        )
        .expect("write response");
        drop(stream);

        let raw = client.join().expect("join");
        assert!(raw.starts_with("HTTP/1.1 500 OK"));
    }

    #[test]
    fn daemon_ui_server_handles_core_and_extension_routes() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);

        let running = Arc::new(AtomicBool::new(true));
        let auth = ControlPlaneAuth::ephemeral();
        let token = auth.token().to_string();
        let server = start_daemon_ui_server(
            temp.path().to_path_buf(),
            "127.0.0.1:0".to_string(),
            Arc::clone(&running),
            auth,
        )
        .expect("start daemon ui");
        let addr = parse_http_url(&server.url);
        let auth_headers = [(UI_AUTH_HEADER, token.as_str())];

        let (status_root, body_root) = http_request(&addr, "GET", "/", None);
        assert_eq!(status_root, 200);
        assert!(body_root.contains("Copper Settings"));

        let (status_descriptor, body_descriptor) =
            http_request_with_headers(&addr, "GET", "/descriptor", &auth_headers, None);
        assert_eq!(status_descriptor, 200);
        assert!(body_descriptor.contains("desktop-torrent-organizer"));

        let (status_core_get, body_core_get) =
            http_request_with_headers(&addr, "GET", "/config/core", &auth_headers, None);
        assert_eq!(status_core_get, 200);
        assert!(body_core_get.contains("{"));

        let (status_core_post, body_core_post) = http_request_with_headers(
            &addr,
            "POST",
            "/config/core",
            &auth_headers,
            Some(r#"{"uiTheme":"dark"}"#),
        );
        assert_eq!(status_core_post, 200);
        assert!(body_core_post.contains("\"ok\": true"));

        let (status_core_bad, body_core_bad) = http_request_with_headers(
            &addr,
            "POST",
            "/config/core",
            &auth_headers,
            Some(r#"["not-object"]"#),
        );
        assert_eq!(status_core_bad, 400);
        assert!(body_core_bad.contains("JSON object"));

        let (status_info_core, body_info_core) =
            http_request_with_headers(&addr, "GET", "/info/core", &auth_headers, None);
        assert_eq!(status_info_core, 200);
        assert!(body_info_core.contains("runtimeExtensionRoots"));

        let ext_path = "/config/extension/desktop-torrent-organizer";
        let (status_ext_get, _) =
            http_request_with_headers(&addr, "GET", ext_path, &auth_headers, None);
        assert_eq!(status_ext_get, 200);

        let (status_ext_post, body_ext_post) = http_request_with_headers(
            &addr,
            "POST",
            ext_path,
            &auth_headers,
            Some(r#"{"desktopFolder":"D:/Desktop"}"#),
        );
        assert_eq!(status_ext_post, 200);
        assert!(body_ext_post.contains("\"ok\": true"));

        let (status_info_ext, body_info_ext) = http_request_with_headers(
            &addr,
            "GET",
            "/info/extension/desktop-torrent-organizer",
            &auth_headers,
            None,
        );
        assert_eq!(status_info_ext, 200);
        assert!(body_info_ext.contains("\"commands\""));

        let (status_missing_ext_get, _) = http_request_with_headers(
            &addr,
            "GET",
            "/config/extension/missing",
            &auth_headers,
            None,
        );
        assert_eq!(status_missing_ext_get, 404);

        let (status_missing_ext_post, _) = http_request_with_headers(
            &addr,
            "POST",
            "/config/extension/missing",
            &auth_headers,
            Some(r#"{"x":1}"#),
        );
        assert_eq!(status_missing_ext_post, 404);

        let (status_close, body_close) =
            http_request_with_headers(&addr, "POST", "/close", &auth_headers, Some("{}"));
        assert_eq!(status_close, 400);
        assert!(body_close.contains("close is disabled"));

        let (status_not_found, _) =
            http_request_with_headers(&addr, "GET", "/does-not-exist", &auth_headers, None);
        assert_eq!(status_not_found, 404);

        running.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(80));
    }

    #[test]
    fn daemon_ui_server_rejects_unauthorized_routes() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);

        let running = Arc::new(AtomicBool::new(true));
        let server = start_daemon_ui_server(
            temp.path().to_path_buf(),
            "127.0.0.1:0".to_string(),
            Arc::clone(&running),
            ControlPlaneAuth::ephemeral(),
        )
        .expect("start daemon ui");
        let addr = parse_http_url(&server.url);

        let (status_descriptor, body_descriptor) = http_request(&addr, "GET", "/descriptor", None);
        assert_eq!(status_descriptor, 403);
        assert!(body_descriptor.contains("control-plane token"));

        let (status_post, body_post) = http_request(
            &addr,
            "POST",
            "/config/core",
            Some(r#"{"uiTheme":"dark"}"#),
        );
        assert_eq!(status_post, 403);
        assert!(body_post.contains("control-plane token"));

        running.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(80));
    }

    #[test]
    fn open_extension_config_closes_on_close_route() {
        let temp = tempdir().expect("tempdir");
        let descriptor = sample_descriptor();
        write_extension(temp.path(), &descriptor);

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);
        let bind = format!("127.0.0.1:{}", addr.port());

        let extensions_dir = temp.path().to_path_buf();
        let bind_for_thread = bind.clone();
        let handle = thread::spawn(move || {
            super::open_extension_config(
                &extensions_dir,
                "desktop-torrent-organizer",
                UiOpenOptions {
                    bind_addr: bind_for_thread,
                    open_browser: false,
                    open_window: false,
                    idle_timeout: Duration::from_secs(2),
                },
            )
        });

        let mut up = false;
        for _ in 0..20 {
            if TcpStream::connect(&bind).is_ok() {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(up, "config UI should accept connections");

        let (_, root_body) = http_request(&bind, "GET", "/", None);
        let token = extract_auth_token(&root_body);
        let auth_headers = [(UI_AUTH_HEADER, token.as_str())];
        let (status_close, _) =
            http_request_with_headers(&bind, "POST", "/close", &auth_headers, Some("{}"));
        assert_eq!(status_close, 204);

        let url = handle
            .join()
            .expect("join")
            .expect("open extension config should succeed");
        assert!(url.starts_with("http://127.0.0.1:"));
    }
