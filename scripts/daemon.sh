#!/usr/bin/env bash
set -euo pipefail

action="${1:-run}"
bind_addr="${2:-127.0.0.1:4765}"
extensions_dir="${3:-./extensions}"
reload_interval_ms="${4:-3000}"
extension_id="${5:-}"
action_id="${6:-}"
ui_idle_timeout_ms="${7:-300000}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

case "$action" in
  run)
    cargo run -p copperd -- run --extensions-dir "$extensions_dir" --bind-addr "$bind_addr" --reload-interval-ms "$reload_interval_ms"
    ;;
  health)
    cargo run -p copperd -- daemon health --bind-addr "$bind_addr"
    ;;
  list)
    cargo run -p copperd -- daemon list --bind-addr "$bind_addr"
    ;;
  trigger)
    if [[ -z "$extension_id" ]]; then
      echo "extension_id is required for trigger" >&2
      exit 1
    fi
    if [[ -n "$action_id" ]]; then
      cargo run -p copperd -- daemon trigger "$extension_id" --action "$action_id" --bind-addr "$bind_addr"
    else
      cargo run -p copperd -- daemon trigger "$extension_id" --bind-addr "$bind_addr"
    fi
    ;;
  reload)
    cargo run -p copperd -- daemon reload --bind-addr "$bind_addr"
    ;;
  verify)
    cargo run -p copperd -- daemon verify --bind-addr "$bind_addr"
    ;;
  shutdown)
    cargo run -p copperd -- daemon shutdown --bind-addr "$bind_addr"
    ;;
  ui-open)
    if [[ -z "$extension_id" ]]; then
      echo "extension_id is required for ui-open" >&2
      exit 1
    fi
    cargo run -p copperd -- ui open --extension "$extension_id" --extensions-dir "$extensions_dir" --idle-timeout-ms "$ui_idle_timeout_ms"
    ;;
  *)
    echo "unknown action: $action" >&2
    exit 1
    ;;
esac
