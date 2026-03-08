# Extension Config UI Access

## Goal

Provide a stable, user-facing way to open an extension configuration UI without requiring terminal-only workflows.

## Proposed Access Path

1. Tray menu entry:
   - `Copper -> Extensions -> <Extension Name> -> Configure`
2. Optional command parity:
   - `copperd ui open --extension <extension-id>`
3. UI behavior:
   - Host reads `manifest.json` inputs and renders a form.
   - Submitting form triggers selected extension action with input payload.
   - Stored config is persisted under extension store namespace.

## Why This Works

- Uses already-defined descriptor inputs (`text`, `select`, `folder-picker`, `file-picker`, etc.).
- Keeps extension authoring declarative and AI-friendly.
- Avoids hardcoding per-extension UI.

## Desktop Torrent Organizer Example

Extension id: `desktop-torrent-organizer`

Recommended config actions in UI:

1. `move-torrents`
   - `desktopFolder` (default `~/Desktop`)
   - `torrentsFolder` (default `~/Desktop/Torrents`)
2. `add-extension`
   - `extensionPackage` (zip or tar.gz)
   - `extensionsInstallDir` (default `~/.Copper/extensions`)
3. `show-config`
   - Shows last run + install history

## Current State (2026-03-08)

- Daemon-hosted config UI is always on while daemon runs:
  - `http://127.0.0.1:4766`
- Implemented command: `copperd ui open --extension <id>` (standalone temporary UI server mode).
- Tray shortcut implemented for `desktop-torrent-organizer`:
  - `Configure Desktop Torrent Organizer`
- Extension config + runtime info are stored together at:
  - `~/.Copper/extensions/<extension-id>/data.json`
- UI now has a dedicated **Info** panel per section (`Copper` and each extension).
- Runtime execution of `main.ts` from saved config remains future work.

