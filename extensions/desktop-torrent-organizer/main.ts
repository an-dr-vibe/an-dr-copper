import type { Api, FileEntry } from "@host/api";

const CONFIG_KEY = "desktop-torrent-organizer/config";
const LAST_RUN_KEY = "desktop-torrent-organizer/last-run";

type Inputs = Record<string, unknown>;

type Config = {
  desktopFolder: string;
  torrentsFolder: string;
};

function normalizeConfig(inputs: Inputs): Config {
  return {
    desktopFolder: String(inputs.desktopFolder ?? "~/Desktop"),
    torrentsFolder: String(inputs.torrentsFolder ?? "~/Desktop/Torrents")
  };
}

function hasTorrentExtension(file: FileEntry): boolean {
  return !file.isDir && file.name.toLowerCase().endsWith(".torrent");
}

function joinPath(base: string, name: string): string {
  if (base.endsWith("/") || base.endsWith("\\")) {
    return `${base}${name}`;
  }
  const separator = base.includes("\\") ? "\\" : "/";
  return `${base}${separator}${name}`;
}

function psQuote(value: string): string {
  return value.replace(/'/g, "''");
}

async function ensureFolder(api: Api, path: string): Promise<void> {
  const pwsh = await api.shell.which("pwsh");
  if (pwsh) {
    await api.shell.run(pwsh, [
      "-NoProfile",
      "-Command",
      `New-Item -ItemType Directory -Path '${psQuote(path)}' -Force | Out-Null`
    ]);
    return;
  }

  throw new Error("No shell runtime available to create destination folder");
}

async function moveTorrents(api: Api, config: Config): Promise<void> {
  await ensureFolder(api, config.torrentsFolder);

  const files = await api.fs.list(config.desktopFolder);
  const torrents = files.filter(hasTorrentExtension);

  let moved = 0;
  let failed = 0;
  for (const file of torrents) {
    try {
      await api.fs.move(file.path, joinPath(config.torrentsFolder, file.name));
      moved += 1;
    } catch {
      failed += 1;
    }
  }

  await api.store.set(LAST_RUN_KEY, {
    at: new Date().toISOString(),
    desktopFolder: config.desktopFolder,
    torrentsFolder: config.torrentsFolder,
    found: torrents.length,
    moved,
    failed
  });

  await api.ui.show({
    type: "toast",
    message: `Desktop torrents: moved=${moved}, failed=${failed}`
  });
  await api.notify(`Desktop torrents complete (${moved}/${torrents.length})`);
}

async function showConfig(api: Api, config: Config): Promise<void> {
  const lastRun = (await api.store.get<Record<string, unknown>>(LAST_RUN_KEY)) ?? null;
  await api.ui.show({
    type: "detail",
    title: "Desktop Torrent Organizer",
    content: {
      config,
      lastRun
    }
  });
}

export default function (api: Api) {
  return {
    async onTrigger(inputs: Inputs = {}) {
      const config = normalizeConfig(inputs);
      await api.store.set(CONFIG_KEY, config);

      const action = String(inputs.action ?? "move-torrents");
      if (action === "move-torrents") {
        await moveTorrents(api, config);
        return;
      }
      if (action === "show-config") {
        await showConfig(api, config);
        return;
      }

      await api.ui.show({
        type: "toast",
        message: `Unknown action '${action}'`
      });
    }
  };
}
