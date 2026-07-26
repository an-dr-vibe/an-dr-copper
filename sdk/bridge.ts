// Copper host bridge for Deno subprocess execution.
// Injected as the Deno entry point; loads the extension from COPPER_MAIN_TS.
// stdout carries JSON-RPC requests TO the host; stdin carries JSON-RPC responses FROM the host.

const _enc = new TextEncoder();
const _dec = new TextDecoder();

// Redirect console.log to stderr — stdout is the JSON-RPC channel.
console.log = (...args: unknown[]) => console.error("[ext]", ...args);

let _nextId = 1;
const _pending = new Map<
  number,
  { resolve: (v: unknown) => void; reject: (e: Error) => void }
>();

function _rpc(method: string, params: Record<string, unknown>): Promise<unknown> {
  const id = _nextId++;
  return new Promise((resolve, reject) => {
    _pending.set(id, { resolve, reject });
    Deno.stdout.writeSync(_enc.encode(JSON.stringify({ id, method, params }) + "\n"));
  });
}

async function _stdinLoop(): Promise<void> {
  const reader = Deno.stdin.readable.getReader();
  let buf = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += _dec.decode(value, { stream: true });
      let nl: number;
      while ((nl = buf.indexOf("\n")) !== -1) {
        const line = buf.slice(0, nl).trim();
        buf = buf.slice(nl + 1);
        if (!line) continue;
        try {
          const msg = JSON.parse(line) as {
            id: number;
            result?: unknown;
            error?: string;
          };
          const entry = _pending.get(msg.id);
          if (entry) {
            _pending.delete(msg.id);
            if (msg.error !== undefined) entry.reject(new Error(msg.error));
            else entry.resolve(msg.result);
          }
        } catch {
          // malformed response line — ignore
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

function _toFileUrl(path: string): string {
  if (path.startsWith("file://")) return path;
  // Strip Windows extended-path prefix \\?\ before converting
  const stripped = path.startsWith("\\\\?\\") ? path.slice(4) : path;
  const normalized = stripped.replace(/\\/g, "/");
  // Windows absolute path: C:/foo/bar.ts → file:///C:/foo/bar.ts
  return /^[A-Za-z]:\//.test(normalized)
    ? `file:///${normalized}`
    : `file://${normalized}`;
}

const _mainTs = Deno.env.get("COPPER_MAIN_TS") ?? "";
const _inputs = JSON.parse(Deno.env.get("COPPER_INPUTS") ?? "{}");
const _isWindows = Deno.build.os === "windows";

// Build the host API object surfaced to extensions.
const _api: Record<string, unknown> = {
  fs: {
    list: (path: string) => _rpc("fs.list", { path }),
    move: (src: string, dst: string) => _rpc("fs.move", { src, dst }),
    delete: (path: string) => _rpc("fs.delete", { path }),
  },
  shell: {
    run: (cmd: string, args: string[] = []) => _rpc("shell.run", { cmd, args }),
    which: (binary: string) => _rpc("shell.which", { binary }),
  },
  notify: (message: string) => _rpc("notify", { message }),
  store: {
    get: (key: string) => _rpc("store.get", { key }),
    set: (key: string, value: unknown) => _rpc("store.set", { key, value }),
  },
  ui: {
    show: (markup: unknown) => _rpc("ui.show", { markup }),
    update: (state: unknown) => _rpc("ui.update", { state }),
  },
  keyboard: {
    typeText: (text: string) => _rpc("keyboard.typeText", { text }),
    sendKey: (key: string) => _rpc("keyboard.sendKey", { key }),
    sendCombo: (combo: string) => _rpc("keyboard.sendCombo", { combo }),
    normalizeCombo: (combo: string) => _rpc("keyboard.normalizeCombo", { combo }),
  },
  secureStore: {
    get: (service: string, key: string) => _rpc("secureStore.get", { service, key }),
    set: (service: string, key: string, value: string) =>
      _rpc("secureStore.set", { service, key, value }),
    delete: (service: string, key: string) => _rpc("secureStore.delete", { service, key }),
  },
};

if (_isWindows) {
  _api.windows = {
    display: {
      status: () => _rpc("windows.display.status", {}),
      toggleTaskbarAutoHide: () => _rpc("windows.display.toggleTaskbarAutoHide", {}),
      setTaskbarAutoHide: (autoHide: boolean) =>
        _rpc("windows.display.setTaskbarAutoHide", { autoHide }),
      setResolution: (width: number, height: number, refreshRate: number) =>
        _rpc("windows.display.setResolution", { width, height, refreshRate }),
      setScale: (scalePercent: number) =>
        _rpc("windows.display.setScale", { scalePercent }),
    },
  };
}

// Start the stdin response listener — runs concurrently via Deno event loop.
const _stdinDone = _stdinLoop();

try {
  const mod = await import(_toFileUrl(_mainTs));
  const factory = mod.default ?? mod;
  const instance = typeof factory === "function" ? factory(_api) : factory;

  if (typeof instance?.onLoad === "function") await instance.onLoad();
  await instance.onTrigger(_inputs);
  if (typeof instance?.onUnload === "function") await instance.onUnload();

  Deno.stdout.writeSync(_enc.encode(JSON.stringify({ _done: true }) + "\n"));
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  Deno.stdout.writeSync(_enc.encode(JSON.stringify({ _error: message }) + "\n"));
}

// Wait for stdin to close (host closes it after receiving _done or _error).
await _stdinDone.catch(() => {});
