use crate::config_ui::{self, UiOpenOptions};
use crate::daemon::{self as daemon_runtime, DaemonConfig, IpcRequest, DEFAULT_BIND_ADDR};
use crate::descriptor::{Descriptor, Permission};
use crate::extension::{default_extensions_dir, load_runtime_registry};
use crate::schema::parse_and_validate;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

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
    command: Commands,
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
    /// Validate one descriptor file against the embedded JSON schema
    Validate {
        #[arg(value_name = "DESCRIPTOR")]
        descriptor: PathBuf,
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
    },
    /// Generate a starter main.ts from a descriptor
    GenerateMain {
        #[arg(value_name = "DESCRIPTOR")]
        descriptor: PathBuf,
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
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
        #[arg(long)]
        no_browser: bool,
    },
}

pub fn run() -> Result<(), CliError> {
    let args = Args::parse();
    match args.command {
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
        Commands::Validate { descriptor } => cmd_validate(&descriptor),
        Commands::List { extensions_dir } => cmd_list(&extensions_dir),
        Commands::Verify { extensions_dir } => cmd_verify(&extensions_dir),
        Commands::Trigger {
            id,
            action,
            extensions_dir,
        } => cmd_trigger(&extensions_dir, &id, action.as_deref()),
        Commands::GenerateMain { descriptor, output } => cmd_generate_main(&descriptor, output),
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
        } => {
            let options = UiOpenOptions {
                bind_addr: "127.0.0.1:0".to_string(),
                open_browser: !no_browser,
                idle_timeout: Duration::from_millis(idle_timeout_ms),
            };
            let url = config_ui::open_extension_config(&extensions_dir, &extension, options)?;
            println!("Config UI available at {url}");
            println!(
                "Config file: {}",
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".Copper")
                    .join("ui-config")
                    .join(format!("{extension}.json"))
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
        if !ext.main_ts_path.exists() {
            return Err(CliError::Message(format!(
                "extension {} is missing main.ts",
                ext.descriptor.id
            )));
        }
    }
    println!("Verified {} extension(s) in {}", found, dir.display());
    Ok(())
}

fn cmd_trigger(dir: &Path, id: &str, action: Option<&str>) -> Result<(), CliError> {
    let registry = load_runtime_registry(dir)?;
    let ext = registry
        .get(id)
        .ok_or_else(|| CliError::Message(format!("extension '{}' not found", id)))?;
    let selected_action = if let Some(action_id) = action {
        ext.descriptor
            .actions
            .iter()
            .find(|a| a.id == action_id)
            .ok_or_else(|| {
                CliError::Message(format!(
                    "action '{}' not found in extension '{}'",
                    action_id, id
                ))
            })?
    } else {
        ext.descriptor.actions.first().ok_or_else(|| {
            CliError::Message(format!("extension '{}' contains no executable actions", id))
        })?
    };

    println!(
        "Trigger dry-run: extension='{}' action='{}'",
        ext.descriptor.id, selected_action.id
    );
    println!(
        "Permissions: {}",
        format_permissions(&ext.descriptor.permissions)
    );
    println!("Script:");
    println!("{}", selected_action.script);
    Ok(())
}

fn cmd_generate_main(descriptor_path: &Path, output: Option<PathBuf>) -> Result<(), CliError> {
    let raw = fs::read_to_string(descriptor_path)?;
    let descriptor = parse_and_validate(&raw)?;
    let ts = render_main_ts(&descriptor);

    let out = output.unwrap_or_else(|| {
        descriptor_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("main.ts")
    });
    fs::write(&out, ts)?;
    println!("Generated {}", out.display());
    Ok(())
}

fn cmd_doctor() -> Result<(), CliError> {
    let rustc = binary_available("rustc");
    let cargo = binary_available("cargo");
    let deno = binary_available("deno");

    println!(
        "required: rustc={} cargo={}",
        if rustc { "ok" } else { "missing" },
        if cargo { "ok" } else { "missing" }
    );
    println!(
        "optional: deno={} (needed only when executing TypeScript extensions)",
        if deno { "ok" } else { "missing" }
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

fn format_permissions(perms: &[Permission]) -> String {
    if perms.is_empty() {
        return "none".to_string();
    }
    perms
        .iter()
        .map(|p| match p {
            Permission::Fs => "fs",
            Permission::Shell => "shell",
            Permission::Network => "network",
            Permission::Store => "store",
            Permission::Ui => "ui",
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

fn render_main_ts(descriptor: &Descriptor) -> String {
    let first_action = descriptor
        .actions
        .first()
        .map(|a| a.script.clone())
        .unwrap_or_default();
    format!(
        r#"import type {{ Api }} from "@host/api";

export default function(api: Api) {{
  return {{
    onLoad() {{}},

    async onTrigger(inputs: Record<string, unknown> = {{}}) {{
      {first_action}
      await api.notify("'{name}' completed");
    }},

    onUnload() {{}}
  }};
}}
"#,
        first_action = indent_script(&first_action),
        name = descriptor.name.replace('\'', "")
    )
}

fn indent_script(script: &str) -> String {
    script
        .lines()
        .map(|line| format!("      {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{format_permissions, render_main_ts};
    use crate::descriptor::{Action, Descriptor, Permission};

    #[test]
    fn generate_main_contains_notify_call() {
        let descriptor = Descriptor {
            schema: None,
            id: "test-ext".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            trigger: "test".to_string(),
            permissions: vec![],
            inputs: vec![],
            actions: vec![Action {
                id: "run".to_string(),
                label: "Run".to_string(),
                script: "const value = 1;".to_string(),
            }],
            ui: None,
        };
        let generated = render_main_ts(&descriptor);
        assert!(generated.contains("api.notify"));
        assert!(generated.contains("const value = 1;"));
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
    fn generate_main_indents_multiline_script() {
        let descriptor = Descriptor {
            schema: None,
            id: "test-ext".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            trigger: "test".to_string(),
            permissions: vec![],
            inputs: vec![],
            actions: vec![Action {
                id: "run".to_string(),
                label: "Run".to_string(),
                script: "const first = 1;\nconst second = 2;".to_string(),
            }],
            ui: None,
        };

        let generated = render_main_ts(&descriptor);
        assert!(generated.contains("      const first = 1;"));
        assert!(generated.contains("      const second = 2;"));
    }
}
