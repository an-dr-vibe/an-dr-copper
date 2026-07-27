use crate::config_ui::{self, UiOpenOptions};
use crate::daemon::{
    self as daemon_runtime, DaemonConfig, IpcRequest, DEFAULT_BIND_ADDR, DEFAULT_RELOAD_INTERVAL_MS,
};
use crate::descriptor::permissions_as_strings;
use crate::extension::{default_extensions_dir, load_runtime_registry};
use crate::schema::parse_and_validate;
use crate::state_store::ExtensionStateStore;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

#[cfg(test)]
use crate::descriptor::Permission;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Validation(#[from] crate::schema::ValidationError),
    #[error(transparent)]
    Extension(#[from] crate::extension::ExtensionError),
    #[error(transparent)]
    Daemon(#[from] crate::daemon::DaemonError),
    #[error(transparent)]
    UiConfig(#[from] crate::config_ui::UiConfigError),
}

#[derive(Parser, Debug)]
#[command(name = "copperd", version, about = "Copper extension host MVP")]
pub struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the long-lived daemon with default settings
    Run {
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
        #[arg(long, value_name = "MS", default_value_t = 3_000)]
        reload_interval_ms: u64,
    },
    /// Validate one manifest file against the embedded JSON schema
    Validate {
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
    },
    /// List all discovered extensions
    List {
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
    },
    /// Verify extension pack and run basic consistency checks
    Verify {
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
    },
    /// Trigger an extension in dry-run mode (prints selected action + permissions)
    Trigger {
        #[arg(value_name = "EXTENSION_ID")]
        id: String,
        #[arg(long, value_name = "ACTION_ID")]
        action: Option<String>,
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
        /// Pass key=value inputs to the extension (repeatable: --input text=hello --input other=val)
        #[arg(long = "input", value_name = "KEY=VALUE")]
        inputs: Vec<String>,
    },
    /// Print environment readiness (required and optional tools)
    Doctor,
    /// Run or control the always-on daemon process
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// Open extension configuration UI
    Ui {
        #[command(subcommand)]
        command: UiCommands,
    },
}

#[derive(Subcommand, Debug)]
enum DaemonCommands {
    /// Start the long-running daemon process
    Run {
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
        #[arg(long, value_name = "MS", default_value_t = 3_000)]
        reload_interval_ms: u64,
    },
    /// Check daemon health
    Health {
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
    /// List extensions known by the running daemon
    List {
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
    /// Trigger an extension through daemon IPC
    Trigger {
        #[arg(value_name = "EXTENSION_ID")]
        id: String,
        #[arg(long, value_name = "ACTION_ID")]
        action: Option<String>,
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
    /// Force daemon registry reload
    Reload {
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
    /// Verify extensions through daemon state
    Verify {
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
    /// Ask daemon to exit gracefully
    Shutdown {
        #[arg(long, value_name = "ADDR", default_value = DEFAULT_BIND_ADDR)]
        bind_addr: String,
    },
}

#[derive(Subcommand, Debug)]
enum UiCommands {
    /// Open local web UI for extension config
    Open {
        #[arg(long, value_name = "EXTENSION_ID")]
        extension: String,
        #[arg(long, value_name = "DIR", default_value_os_t = default_extensions_dir())]
        extensions_dir: PathBuf,
        #[arg(long, value_name = "MS", default_value_t = 300_000)]
        idle_timeout_ms: u64,
        #[arg(long, conflicts_with_all = ["browser", "window"])]
        no_browser: bool,
        #[arg(long, conflicts_with_all = ["browser", "no_browser"])]
        window: bool,
        #[arg(long, conflicts_with_all = ["window", "no_browser"])]
        browser: bool,
    },
}

pub fn run() -> Result<(), CliError> {
    let args = Args::parse();
    let command = args.command.unwrap_or_else(default_run_command);
    run_command(command)
}

fn default_run_command() -> Commands {
    Commands::Run {
        extensions_dir: default_extensions_dir(),
        bind_addr: DEFAULT_BIND_ADDR.to_string(),
        reload_interval_ms: DEFAULT_RELOAD_INTERVAL_MS,
    }
}

fn run_command(command: Commands) -> Result<(), CliError> {
    match command {
        Commands::Run {
            extensions_dir,
            bind_addr,
            reload_interval_ms,
        } => daemon_runtime::run_daemon(DaemonConfig {
            extensions_dir,
            bind_addr,
            reload_interval: Duration::from_millis(reload_interval_ms),
        })
        .map_err(CliError::from),
        Commands::Validate { manifest } => cmd_validate(&manifest),
        Commands::List { extensions_dir } => cmd_list(&extensions_dir),
        Commands::Verify { extensions_dir } => cmd_verify(&extensions_dir),
        Commands::Trigger {
            id,
            action,
            extensions_dir,
            inputs,
        } => cmd_trigger(&extensions_dir, &id, action.as_deref(), &inputs),
        Commands::Doctor => cmd_doctor(),
        Commands::Daemon { command } => cmd_daemon(command),
        Commands::Ui { command } => cmd_ui(command),
    }
}

fn cmd_daemon(command: DaemonCommands) -> Result<(), CliError> {
    match command {
        DaemonCommands::Run {
            extensions_dir,
            bind_addr,
            reload_interval_ms,
        } => daemon_runtime::run_daemon(DaemonConfig {
            extensions_dir,
            bind_addr,
            reload_interval: Duration::from_millis(reload_interval_ms),
        })?,
        DaemonCommands::Health { bind_addr } => {
            print_ipc_response(daemon_runtime::send_request(
                &bind_addr,
                &IpcRequest::Health,
            )?)?;
        }
        DaemonCommands::List { bind_addr } => {
            print_ipc_response(daemon_runtime::send_request(&bind_addr, &IpcRequest::List)?)?;
        }
        DaemonCommands::Trigger {
            id,
            action,
            bind_addr,
        } => {
            print_ipc_response(daemon_runtime::send_request(
                &bind_addr,
                &IpcRequest::Trigger { id, action },
            )?)?;
        }
        DaemonCommands::Reload { bind_addr } => {
            print_ipc_response(daemon_runtime::send_request(
                &bind_addr,
                &IpcRequest::Reload,
            )?)?;
        }
        DaemonCommands::Verify { bind_addr } => {
            print_ipc_response(daemon_runtime::send_request(
                &bind_addr,
                &IpcRequest::Verify,
            )?)?;
        }
        DaemonCommands::Shutdown { bind_addr } => {
            print_ipc_response(daemon_runtime::send_request(
                &bind_addr,
                &IpcRequest::Shutdown,
            )?)?;
        }
    }
    Ok(())
}

fn cmd_ui(command: UiCommands) -> Result<(), CliError> {
    match command {
        UiCommands::Open {
            extension,
            extensions_dir,
            idle_timeout_ms,
            no_browser,
            window,
            browser,
        } => {
            let options = UiOpenOptions {
                bind_addr: "127.0.0.1:0".to_string(),
                open_browser: browser,
                open_window: window || (!browser && !no_browser),
                idle_timeout: Duration::from_millis(idle_timeout_ms),
            };
            let url = config_ui::open_extension_config(&extensions_dir, &extension, options)?;
            println!("Config UI available at {url}");
            println!(
                "Config file: {}",
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".Copper")
                    .join("extensions")
                    .join(&extension)
                    .join("config.json")
                    .display()
            );
        }
    }
    Ok(())
}

fn cmd_validate(path: &Path) -> Result<(), CliError> {
    let raw = fs::read_to_string(path)?;
    let descriptor = parse_and_validate(&raw)?;
    println!(
        "OK: {} ({}) trigger='{}' actions={}",
        descriptor.name,
        descriptor.id,
        descriptor.trigger,
        descriptor.actions.len()
    );
    Ok(())
}

fn cmd_list(dir: &Path) -> Result<(), CliError> {
    let registry = load_runtime_registry(dir)?;
    if registry.list().count() == 0 {
        println!("No extensions discovered in {}", dir.display());
        return Ok(());
    }
    for ext in registry.list() {
        println!(
            "{}\t{}\ttrigger={}\tpermissions={}",
            ext.descriptor.id,
            ext.descriptor.version,
            ext.descriptor.trigger,
            ext.descriptor.permissions.len()
        );
    }
    Ok(())
}

fn cmd_verify(dir: &Path) -> Result<(), CliError> {
    let registry = load_runtime_registry(dir)?;
    let mut found = 0usize;
    for ext in registry.list() {
        found += 1;
        if ext.descriptor.actions.is_empty() {
            return Err(CliError::Message(format!(
                "extension {} has no actions",
                ext.descriptor.id
            )));
        }
        if !ext.runtime_artifact_path().exists() {
            return Err(CliError::Message(format!(
                "extension {} is missing runtime artifact {}",
                ext.descriptor.id,
                ext.runtime_artifact_path().display()
            )));
        }
    }
    println!("Verified {} extension(s) in {}", found, dir.display());
    Ok(())
}

fn cmd_trigger(
    dir: &Path,
    id: &str,
    action: Option<&str>,
    raw_inputs: &[String],
) -> Result<(), CliError> {
    let registry = load_runtime_registry(dir)?;
    let ext = registry
        .get(id)
        .ok_or_else(|| CliError::Message(format!("extension '{}' not found", id)))?;
    let store = ExtensionStateStore::for_current_user()?;
    let selected_action = match action {
        Some(action_id) => ext
            .descriptor
            .actions
            .iter()
            .find(|candidate| candidate.id == action_id)
            .ok_or_else(|| CliError::Message(format!("action '{action_id}' not found")))?,
        None => ext
            .descriptor
            .actions
            .first()
            .ok_or_else(|| CliError::Message("no action defined".to_string()))?,
    };
    let extension_id = ext.descriptor.id.clone();
    let action_id = selected_action.id.clone();
    let permissions = permissions_as_strings(&ext.descriptor.permissions);

    let mut inputs_map = serde_json::Map::new();
    for raw in raw_inputs {
        if let Some((key, val)) = raw.split_once('=') {
            inputs_map.insert(key.to_string(), serde_json::Value::String(val.to_string()));
        }
    }

    println!(
        "Trigger: extension='{}' action='{}' permissions={}",
        extension_id,
        action_id,
        if permissions.is_empty() {
            "none".to_string()
        } else {
            permissions.join(",")
        }
    );

    crate::bones_integration::execute_component_action(
        &registry,
        &store,
        &extension_id,
        &action_id,
        inputs_map,
    )
    .map_err(CliError::Message)?;

    println!("Done: '{extension_id}'");
    Ok(())
}

fn cmd_doctor() -> Result<(), CliError> {
    cmd_doctor_with(binary_available)
}

fn cmd_doctor_with<F>(is_available: F) -> Result<(), CliError>
where
    F: Fn(&str) -> bool,
{
    let rustc = is_available("rustc");
    let cargo = is_available("cargo");

    println!(
        "required: rustc={} cargo={}",
        if rustc { "ok" } else { "missing" },
        if cargo { "ok" } else { "missing" }
    );

    if !rustc || !cargo {
        return Err(CliError::Message(
            "missing required Rust toolchain components".to_string(),
        ));
    }
    Ok(())
}

fn print_ipc_response(response: daemon_runtime::IpcResponse) -> Result<(), CliError> {
    if !response.ok {
        return Err(CliError::Message(response.message));
    }
    println!("{}", response.message);
    if let Some(data) = response.data {
        let pretty = serde_json::to_string_pretty(&data)
            .map_err(|err| CliError::Message(format!("failed to format daemon response: {err}")))?;
        println!("{pretty}");
    }
    Ok(())
}

#[cfg(test)]
fn format_permissions(perms: &[Permission]) -> String {
    if perms.is_empty() {
        return "none".to_string();
    }
    perms
        .iter()
        .map(|p| match p {
            Permission::Fs => "fs",
            Permission::Keyboard => "keyboard",
            Permission::Network => "network",
            Permission::SecureStore => "secure-store",
            Permission::Shell => "shell",
            Permission::Store => "store",
            Permission::Ui => "ui",
            Permission::WindowsDisplay => "windows-display",
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn binary_available(name: &str) -> bool {
    let locator = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    Command::new(locator)
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        binary_available, cmd_daemon, cmd_doctor_with, cmd_list, cmd_trigger, default_run_command,
        format_permissions, print_ipc_response, run_command, Args, Commands, DaemonCommands,
    };
    use crate::bones_integration::execute_component_action;
    use crate::daemon::IpcResponse;
    use crate::descriptor::Permission;
    use crate::extension::Registry;
    use crate::state_store::ExtensionStateStore;
    use clap::Parser;
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tiny_http::{Method, Response as HttpResponse, Server};

    fn write_extension(root: &std::path::Path, id: &str) {
        let ext = root.join(id);
        fs::create_dir_all(&ext).expect("create extension dir");
        fs::write(
            ext.join("manifest.json"),
            format!(
                r#"{{
                    "$schema": "https://Copper.dev/schemas/extension/1.0.0/descriptor.schema.json",
                    "id": "{id}",
                    "name": "Test Extension",
                    "version": "1.0.0",
                    "trigger": "test",
                    "runtime": {{
                        "kind": "wasm-component",
                        "abi": "copper.component/1",
                        "artifact": "{id}.wasm"
                    }},
                    "actions": [
                        {{ "id": "run", "label": "Run", "script": "return;" }}
                    ]
                }}"#
            ),
        )
        .expect("write manifest");
        fs::write(ext.join(format!("{id}.wasm")), b"\0asm").expect("write component");
    }

    fn spawn_ipc_server(expected_op: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let addr = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("local addr").to_string()
        };
        let server_addr = addr.clone();
        let handle = std::thread::spawn(move || {
            let server = Server::http(&server_addr).expect("server");
            let mut request = server.recv().expect("request");
            match expected_op {
                "health" => {
                    assert_eq!(request.method(), &Method::Get);
                    assert_eq!(request.url(), "/health");
                }
                "list" => {
                    assert_eq!(request.method(), &Method::Get);
                    assert_eq!(request.url(), "/list");
                }
                "trigger" => {
                    assert_eq!(request.method(), &Method::Post);
                    assert_eq!(request.url(), "/trigger");
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).expect("body");
                    assert!(body.contains("\"id\":\"alpha-ext\""));
                }
                "reload" => {
                    assert_eq!(request.method(), &Method::Post);
                    assert_eq!(request.url(), "/reload");
                }
                "verify" => {
                    assert_eq!(request.method(), &Method::Post);
                    assert_eq!(request.url(), "/verify");
                }
                "shutdown" => {
                    assert_eq!(request.method(), &Method::Post);
                    assert_eq!(request.url(), "/shutdown");
                }
                other => panic!("unexpected op {other}"),
            }
            request
                .respond(HttpResponse::from_string(r#"{"ok":true,"message":"ok"}"#))
                .expect("write response");
        });
        (addr, handle)
    }

    fn assert_daemon_command_ipc(
        expected_op: &'static str,
        make_command: impl FnOnce(String) -> DaemonCommands,
    ) {
        let (addr, handle) = spawn_ipc_server(expected_op);
        cmd_daemon(make_command(addr)).expect("daemon command should succeed");
        handle.join().expect("join");
    }

    #[test]
    fn format_permissions_handles_empty_and_values() {
        assert_eq!(format_permissions(&[]), "none");
        assert_eq!(
            format_permissions(&[Permission::Fs, Permission::Shell, Permission::Ui]),
            "fs,shell,ui"
        );
    }

    #[test]
    fn binary_available_returns_false_for_missing_command() {
        assert!(!binary_available("definitely-not-a-real-binary-name-12345"));
    }

    #[test]
    fn print_ipc_response_returns_error_when_response_not_ok() {
        let err = print_ipc_response(IpcResponse::err("request failed")).expect_err("must error");
        assert!(err.to_string().contains("request failed"));
    }

    #[test]
    fn print_ipc_response_ok_with_data_formats_pretty_json() {
        let response = IpcResponse::ok("ok", Some(serde_json::json!({ "k": 1 })));
        print_ipc_response(response).expect("ok response should print");
    }

    #[test]
    fn format_permissions_covers_all_variants() {
        let formatted = format_permissions(&[
            Permission::Fs,
            Permission::Shell,
            Permission::Network,
            Permission::Store,
            Permission::Ui,
            Permission::WindowsDisplay,
        ]);
        assert_eq!(formatted, "fs,shell,network,store,ui,windows-display");
    }

    #[test]
    fn cmd_list_reports_empty_directory() {
        let temp = tempdir().expect("tempdir");
        cmd_list(temp.path()).expect("empty list should succeed");
    }

    #[test]
    fn cmd_trigger_errors_for_unknown_action() {
        let temp = tempdir().expect("tempdir");
        write_extension(temp.path(), "alpha-ext");
        let err =
            cmd_trigger(temp.path(), "alpha-ext", Some("missing"), &[]).expect_err("must fail");
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn wasm_trigger_executes_to_capability_quiescence() {
        let extensions = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("extensions");
        let registry = Registry::load_from_dir(&extensions).expect("shipped registry");
        let temp = tempdir().expect("tempdir");
        let store = ExtensionStateStore::new(temp.path().join("state"));

        execute_component_action(
            &registry,
            &store,
            "session-counter",
            "increment",
            Default::default(),
        )
        .expect("component trigger");
        assert_eq!(
            store.load_store("session-counter").expect("state")["session-counter/runs"],
            serde_json::json!(1)
        );
    }

    #[test]
    fn cmd_daemon_health_sends_expected_ipc_request() {
        assert_daemon_command_ipc("health", |bind_addr| DaemonCommands::Health { bind_addr });
    }

    #[test]
    fn cmd_daemon_list_sends_expected_ipc_request() {
        assert_daemon_command_ipc("list", |bind_addr| DaemonCommands::List { bind_addr });
    }

    #[test]
    fn cmd_daemon_trigger_sends_expected_ipc_request() {
        assert_daemon_command_ipc("trigger", |bind_addr| DaemonCommands::Trigger {
            id: "alpha-ext".to_string(),
            action: Some("run".to_string()),
            bind_addr,
        });
    }

    #[test]
    fn cmd_daemon_reload_sends_expected_ipc_request() {
        assert_daemon_command_ipc("reload", |bind_addr| DaemonCommands::Reload { bind_addr });
    }

    #[test]
    fn cmd_daemon_verify_sends_expected_ipc_request() {
        assert_daemon_command_ipc("verify", |bind_addr| DaemonCommands::Verify { bind_addr });
    }

    #[test]
    fn cmd_daemon_shutdown_sends_expected_ipc_request() {
        assert_daemon_command_ipc("shutdown", |bind_addr| DaemonCommands::Shutdown {
            bind_addr,
        });
    }

    #[test]
    fn cmd_daemon_run_returns_error_for_invalid_bind() {
        let err = cmd_daemon(DaemonCommands::Run {
            extensions_dir: PathBuf::from("."),
            bind_addr: "not-an-addr".to_string(),
            reload_interval_ms: 1,
        })
        .expect_err("invalid bind should fail");
        assert!(err.to_string().contains("address"));
    }

    #[test]
    fn run_command_run_returns_error_for_invalid_bind() {
        let err = run_command(Commands::Run {
            extensions_dir: PathBuf::from("."),
            bind_addr: "not-an-addr".to_string(),
            reload_interval_ms: 1,
        })
        .expect_err("invalid bind should fail");
        assert!(err.to_string().contains("address"));
    }

    #[test]
    fn doctor_with_reports_missing_required_toolchain() {
        let err = cmd_doctor_with(|name| name == "cargo").expect_err("must fail");
        assert!(err
            .to_string()
            .contains("missing required Rust toolchain components"));
    }

    #[test]
    fn args_parse_without_subcommand_defaults_to_none() {
        let args = Args::try_parse_from(["copperd"]).expect("parse args");
        assert!(args.command.is_none());
    }

    #[test]
    fn default_run_command_matches_expected_defaults() {
        match default_run_command() {
            Commands::Run {
                bind_addr,
                reload_interval_ms,
                extensions_dir,
            } => {
                assert_eq!(bind_addr, crate::daemon::DEFAULT_BIND_ADDR);
                assert_eq!(
                    reload_interval_ms,
                    crate::daemon::DEFAULT_RELOAD_INTERVAL_MS
                );
                assert!(!extensions_dir.as_os_str().is_empty());
            }
            _ => panic!("default command should be run"),
        }
    }
}
