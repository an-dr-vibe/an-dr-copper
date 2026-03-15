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
   - Settings and runtime status are shown separately.
   - Stored config is persisted under a dedicated extension config file.
   - Extensions may declare `settings.applyActions` so saving the page can also apply the saved config to the live host state.

## Why This Works

- Uses already-defined descriptor inputs (`text`, `select`, `folder-picker`, `file-picker`, etc.).
- Allows richer settings pages through optional `settings.sections`, per-field descriptions, `settings.status` metadata, and `settings.applyActions`.
- Keeps extension authoring declarative and AI-friendly.
- Avoids hardcoding per-extension UI.

## Desktop Torrent Organizer Example

Extension id: `desktop-torrent-organizer`

Recommended config actions in UI:

1. `move-torrents`
   - `desktopFolder` (default `~/Desktop`)
   - `torrentsFolder` (default `~/Desktop/Torrents`)
2. Core package install settings
   - `extensionPackage` (zip or tar.gz)
   - `extensionsInstallDir` (default `~/.Copper/extensions`)
3. `show-config`
   - Shows last run + install history

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
- UI now uses a dedicated extension page with separate **Settings** and **Status** tabs.
- Shared package-install inputs now live on the **Core** settings page instead of inside the desktop torrent extension settings.
- `windows-display-manager` now saves and applies its declared display actions from the config page.
- Runtime execution of `main.ts` from saved config remains future work.

