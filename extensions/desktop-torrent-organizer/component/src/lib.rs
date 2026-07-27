#[cfg(target_family = "wasm")]
use copper_component_sdk::host_api::{log, Level};
#[cfg(target_family = "wasm")]
use copper_component_sdk::{
    decode_host_event, request, Capability, Guest, HostEvent, COPPER_ACTIONS_ENDPOINT,
};
#[cfg(target_family = "wasm")]
use serde_json::json;
#[cfg(target_family = "wasm")]
use serde_json::Map;
use serde_json::Value;
#[cfg(target_family = "wasm")]
use std::cell::RefCell;
#[cfg(target_family = "wasm")]
use std::collections::BTreeMap;
use std::collections::VecDeque;

#[cfg(target_family = "wasm")]
const CONFIG_KEY: &str = "desktop-torrent-organizer/config";
#[cfg(target_family = "wasm")]
const LAST_RUN_KEY: &str = "desktop-torrent-organizer/last-run";
#[cfg(target_family = "wasm")]
const MOVE_ACTION: &str = "move-torrents";
#[cfg(target_family = "wasm")]
const SHOW_ACTION: &str = "show-config";

#[cfg(target_family = "wasm")]
thread_local! {
    static STEPS: RefCell<BTreeMap<String, Step>> = const { RefCell::new(BTreeMap::new()) };
}

#[derive(Debug, Clone)]
struct Config {
    desktop_folder: String,
    torrents_folder: String,
    auto_run: bool,
    poll_interval_seconds: u64,
}

#[derive(Debug, Clone)]
struct TorrentFile {
    name: String,
    path: String,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone, Default)]
struct Report {
    found: u64,
    moved: u64,
    failed: u64,
}

#[cfg(target_family = "wasm")]
#[derive(Debug, Clone)]
enum Step {
    Config {
        action: String,
    },
    ConfigStored {
        action: String,
        config: Config,
    },
    DirectoryReady {
        config: Config,
    },
    Listed {
        config: Config,
    },
    Moving {
        flow_id: String,
        config: Config,
        remaining: VecDeque<TorrentFile>,
        report: Report,
        index: u64,
    },
    Timestamp {
        config: Config,
        report: Report,
    },
    LastRunStored {
        config: Config,
        report: Report,
        timestamp: u64,
    },
    StatusStored {
        report: Report,
    },
    ToastShown {
        report: Report,
    },
    LastRunLoaded {
        config: Config,
    },
    Done,
}

#[cfg(target_family = "wasm")]
struct Component;

#[cfg(target_family = "wasm")]
impl Guest for Component {
    fn init() {
        log(Level::Info, "Desktop Torrent Organizer initialized");
    }

    fn shutdown() {
        STEPS.with(|steps| steps.borrow_mut().clear());
    }

    fn on_tick(_dt: f32) {}

    fn on_message(_topic: String, sender: String, payload: Vec<u8>) -> Option<Vec<u8>> {
        match decode_host_event(&sender, &payload) {
            Ok(HostEvent::Action {
                request_id,
                action_id,
                ..
            }) if sender == COPPER_ACTIONS_ENDPOINT
                && matches!(action_id.as_str(), MOVE_ACTION | SHOW_ACTION) =>
            {
                queue(
                    format!("{request_id}.config"),
                    Step::Config { action: action_id },
                    Capability::Store,
                    "config.get",
                    Map::new(),
                );
            }
            Ok(HostEvent::JobResult {
                request_id, result, ..
            }) => {
                if let Some(step) = take_step(&request_id) {
                    advance(request_id, step, result);
                }
            }
            Ok(HostEvent::JobError {
                request_id,
                code,
                message,
                ..
            }) => {
                if let Some(request_id) = request_id {
                    if let Some(step) = take_step(&request_id) {
                        fail_step(request_id, step, &code, &message);
                    }
                }
                log(Level::Error, &format!("{code}: {message}"));
            }
            Ok(_) => {}
            Err(error) => log(Level::Warn, &error.to_string()),
        }
        None
    }
}

#[cfg(target_family = "wasm")]
fn advance(request_id: String, step: Step, result: Value) {
    match step {
        Step::Config { action } => {
            let config = normalize_config(&result);
            queue(
                format!("{request_id}.store"),
                Step::ConfigStored {
                    action,
                    config: config.clone(),
                },
                Capability::Store,
                "set",
                map([("key", json!(CONFIG_KEY)), ("value", config_value(&config))]),
            );
        }
        Step::ConfigStored { action, config } if action == MOVE_ACTION => {
            queue(
                format!("{request_id}.mkdir"),
                Step::DirectoryReady {
                    config: config.clone(),
                },
                Capability::Fs,
                "create-dir",
                map([("path", json!(config.torrents_folder))]),
            );
        }
        Step::ConfigStored { config, .. } => {
            queue(
                format!("{request_id}.last-run"),
                Step::LastRunLoaded { config },
                Capability::Store,
                "get",
                map([("key", json!(LAST_RUN_KEY))]),
            );
        }
        Step::DirectoryReady { config } => {
            queue(
                format!("{request_id}.list"),
                Step::Listed {
                    config: config.clone(),
                },
                Capability::Fs,
                "list",
                map([("path", json!(config.desktop_folder))]),
            );
        }
        Step::Listed { config } => {
            let remaining = torrent_files(&result);
            let report = Report {
                found: remaining.len() as u64,
                ..Report::default()
            };
            continue_moves(request_id, config, remaining, report, 0);
        }
        Step::Moving {
            flow_id,
            config,
            remaining,
            mut report,
            index,
        } => {
            report.moved = report.moved.saturating_add(1);
            continue_moves(flow_id, config, remaining, report, index);
        }
        Step::Timestamp { config, report } => {
            let timestamp = result.as_u64().unwrap_or_default();
            queue(
                format!("{request_id}.last-run"),
                Step::LastRunStored {
                    config: config.clone(),
                    report: report.clone(),
                    timestamp,
                },
                Capability::Store,
                "set",
                map([
                    ("key", json!(LAST_RUN_KEY)),
                    (
                        "value",
                        json!({
                            "atUnix": timestamp,
                            "desktopFolder": config.desktop_folder,
                            "torrentsFolder": config.torrents_folder,
                            "found": report.found,
                            "moved": report.moved,
                            "failed": report.failed
                        }),
                    ),
                ]),
            );
        }
        Step::LastRunStored {
            config,
            report,
            timestamp,
        } => {
            let mut status = json!({
                "autoRun": config.auto_run,
                "pollIntervalSeconds": config.poll_interval_seconds,
                "desktopFolder": config.desktop_folder,
                "torrentsFolder": config.torrents_folder,
                "lastScanUnix": timestamp,
                "lastScanFound": report.found,
                "lastScanMoved": report.moved,
                "lastScanFailed": report.failed
            });
            if report.moved > 0 {
                status["lastMoveUnix"] = json!(timestamp);
            }
            queue(
                format!("{request_id}.status"),
                Step::StatusStored {
                    report: report.clone(),
                },
                Capability::Store,
                "status.merge",
                map([("value", status)]),
            );
        }
        Step::StatusStored { report } => {
            queue(
                format!("{request_id}.toast"),
                Step::ToastShown {
                    report: report.clone(),
                },
                Capability::Ui,
                "show",
                map([(
                    "markup",
                    json!({
                        "type": "toast",
                        "message": format!(
                            "Desktop torrents: moved={}, failed={}",
                            report.moved, report.failed
                        )
                    }),
                )]),
            );
        }
        Step::ToastShown { report } => {
            queue(
                format!("{request_id}.notify"),
                Step::Done,
                Capability::Notify,
                "show",
                map([(
                    "message",
                    json!(format!(
                        "Desktop torrents complete ({}/{})",
                        report.moved, report.found
                    )),
                )]),
            );
        }
        Step::LastRunLoaded { config } => {
            queue(
                format!("{request_id}.detail"),
                Step::Done,
                Capability::Ui,
                "show",
                map([(
                    "markup",
                    json!({
                        "type": "detail",
                        "title": "Desktop Torrent Organizer",
                        "content": {
                            "config": config_value(&config),
                            "lastRun": result
                        }
                    }),
                )]),
            );
        }
        Step::Done => {}
    }
}

#[cfg(target_family = "wasm")]
fn fail_step(_request_id: String, step: Step, _code: &str, _message: &str) {
    if let Step::Moving {
        flow_id,
        config,
        remaining,
        mut report,
        index,
    } = step
    {
        report.failed = report.failed.saturating_add(1);
        continue_moves(flow_id, config, remaining, report, index);
    }
}

#[cfg(target_family = "wasm")]
fn continue_moves(
    flow_id: String,
    config: Config,
    mut remaining: VecDeque<TorrentFile>,
    report: Report,
    index: u64,
) {
    if let Some(file) = remaining.pop_front() {
        let next_index = index.saturating_add(1);
        queue(
            format!("{flow_id}.move.{next_index}"),
            Step::Moving {
                flow_id: flow_id.clone(),
                config: config.clone(),
                remaining,
                report,
                index: next_index,
            },
            Capability::Fs,
            "move",
            map([
                ("src", json!(file.path)),
                ("dst", json!(join_path(&config.torrents_folder, &file.name))),
            ]),
        );
    } else {
        queue(
            format!("{flow_id}.time"),
            Step::Timestamp { config, report },
            Capability::Clock,
            "unix-now",
            Map::new(),
        );
    }
}

fn normalize_config(value: &Value) -> Config {
    Config {
        desktop_folder: value
            .get("desktopFolder")
            .and_then(Value::as_str)
            .unwrap_or("~/Desktop")
            .to_string(),
        torrents_folder: value
            .get("torrentsFolder")
            .and_then(Value::as_str)
            .unwrap_or("~/Desktop/Torrents")
            .to_string(),
        auto_run: value
            .get("autoRun")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        poll_interval_seconds: value
            .get("pollIntervalSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 86_400),
    }
}

fn torrent_files(value: &Value) -> VecDeque<TorrentFile> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|file| !file.get("isDir").and_then(Value::as_bool).unwrap_or(true))
        .filter_map(|file| {
            let name = file.get("name")?.as_str()?;
            if !name.to_ascii_lowercase().ends_with(".torrent") {
                return None;
            }
            Some(TorrentFile {
                name: name.to_string(),
                path: file.get("path")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn join_path(base: &str, name: &str) -> String {
    if base.ends_with('/') || base.ends_with('\\') {
        return format!("{base}{name}");
    }
    let separator = if base.contains('\\') { '\\' } else { '/' };
    format!("{base}{separator}{name}")
}

#[cfg(target_family = "wasm")]
fn config_value(config: &Config) -> Value {
    json!({
        "desktopFolder": config.desktop_folder,
        "torrentsFolder": config.torrents_folder
    })
}

#[cfg(target_family = "wasm")]
fn queue(
    request_id: String,
    step: Step,
    capability: Capability,
    operation: &str,
    args: Map<String, Value>,
) {
    STEPS.with(|steps| {
        steps.borrow_mut().insert(request_id.clone(), step);
    });
    if let Err(error) = request(request_id.clone(), capability, operation, args) {
        take_step(&request_id);
        log(Level::Error, &error.to_string());
    }
}

#[cfg(target_family = "wasm")]
fn take_step(request_id: &str) -> Option<Step> {
    STEPS.with(|steps| steps.borrow_mut().remove(request_id))
}

#[cfg(target_family = "wasm")]
fn map<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(target_family = "wasm")]
copper_component_sdk::bindings::export!(
    Component with_types_in copper_component_sdk::bindings
);

#[cfg(test)]
mod tests {
    use super::{join_path, normalize_config, torrent_files};
    use serde_json::json;

    #[test]
    fn config_defaults_and_path_joining_are_cross_platform() {
        let config = normalize_config(&json!({}));
        assert_eq!(config.desktop_folder, "~/Desktop");
        assert_eq!(config.torrents_folder, "~/Desktop/Torrents");
        assert!(config.auto_run);
        assert_eq!(config.poll_interval_seconds, 5);
        assert_eq!(
            join_path("C:\\Desktop\\Torrents", "one.torrent"),
            "C:\\Desktop\\Torrents\\one.torrent"
        );
        assert_eq!(
            join_path("/tmp/torrents", "one.torrent"),
            "/tmp/torrents/one.torrent"
        );
    }

    #[test]
    fn only_regular_torrent_files_are_selected() {
        let files = torrent_files(&json!([
            {"name": "one.torrent", "path": "/one.torrent", "isDir": false},
            {"name": "TWO.TORRENT", "path": "/TWO.TORRENT", "isDir": false},
            {"name": "three.txt", "path": "/three.txt", "isDir": false},
            {"name": "folder.torrent", "path": "/folder.torrent", "isDir": true}
        ]));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "one.torrent");
        assert_eq!(files[1].path, "/TWO.TORRENT");
    }
}
