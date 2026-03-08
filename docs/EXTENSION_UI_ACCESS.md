# Extension Config UI Access

## Goal

Provide a stable, user-facing way to open an extension configuration UI without requiring terminal-only workflows.

## Proposed Access Path

1. Tray menu entry:
   - `Copper -> Extensions -> <Extension Name> -> Configure`
2. Optional command parity:
   - `copperd ui open --extension <extension-id>`
3. UI behavior:
   - Host reads `descriptor.json` inputs and renders a form.
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

- Descriptor + extension actions are implemented.
- Daemon/UI bridge command (`ui open`) is not yet implemented.
- Until UI open is wired, extension actions can be inspected/triggered through CLI/daemon commands.
