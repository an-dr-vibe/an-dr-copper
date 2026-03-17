# Extension Config UI Access

## Goal

Provide a stable, user-facing way to open an extension configuration UI without requiring terminal-only workflows.

## Proposed Access Path

1. Tray menu entry:
   - `Copper -> Extensions -> <Extension Name> -> Configure`
2. Optional command parity:
   - `copperd ui open --extension <extension-id>`
3. UI behavior:
   - Host reads `manifest.json` inputs and optional `settings` metadata and renders a dedicated extension settings page.
   - If `settings.tabs` is declared, the page renders those user-defined tabs.
   - If no tabs are declared, the page renders a single combined view without a tab strip.
   - Extension pages render settings and status only; manifest actions are surfaced centrally from **Core -> Extensions**.
   - Stored config is persisted under a dedicated extension config file.
   - Extensions may declare `settings.applyActions` so saving the page can also apply the saved config to the live host state.

## Why This Works

- Uses already-defined descriptor inputs (`text`, `select`, `list-select`, `folder-picker`, `file-picker`, etc.).
- Allows richer settings pages through optional `settings.sections`, `settings.tabs`, per-field descriptions, `settings.status` metadata, and `settings.applyActions`.
- Keeps extension authoring declarative and AI-friendly.
- Avoids hardcoding per-extension UI.
- The shared renderer highlights unsaved changes at the field/card level and upgrades the save button state while edits are pending.

## Desktop Torrent Organizer Example

Extension id: `desktop-torrent-organizer`

Recommended config actions in UI:

1. `move-torrents`
   - `desktopFolder` (default `~/Desktop`)
   - `torrentsFolder` (default `~/Desktop/Torrents`)
2. `show-config`
   - Shows the saved monitor configuration and last run summary

## Current State (2026-03-15)

- Daemon-hosted config UI is always on while daemon runs:
  - `http://127.0.0.1:4766`
- Implemented command: `copperd ui open --extension <id>` (standalone temporary UI server mode).
- Tray shortcut implemented for `desktop-torrent-organizer`:
  - `Configure Desktop Torrent Organizer`
- Extension config is stored at:
  - `~/.Copper/extensions/<extension-id>/config.json`
- Extension runtime status is stored at:
  - `~/.Copper/extensions/<extension-id>/status.json`
- Legacy `data.json` is still read as a fallback during migration.
- UI now uses dedicated extension pages with optional manifest-defined tabs.
- Core settings use fixed tabs for **General**, **Package Install**, and **Extensions**.
- Core **Extensions** renders discoverable extensions as cards with enable/disable controls, a settings shortcut, and lazy-loaded command help generated from manifest actions.
- Shared package-install inputs now live on the **Core** settings page instead of inside the desktop torrent extension settings.
- Core settings now include **Launch Copper at login**, which applies user-level autostart registration when saved. On Windows, the autostart path prefers the `copper.exe` GUI launcher so it does not open a terminal window at login.
- Core settings also include per-extension enable/disable toggles backed by `~/.Copper/extensions/copper-core/config.json` `disabledExtensions`.
- Legacy `~/.Copper/extensions/copper-core/data.json` is still read as a fallback during migration.
- Platform-restricted extensions remain visible in the UI with their supported-platform metadata even when the current host cannot run them.
- `windows-display-manager` now saves and applies its declared display actions from the config page.
- Runtime execution of `main.ts` from saved config remains future work.

