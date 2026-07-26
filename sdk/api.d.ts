export type Permission = "fs" | "keyboard" | "network" | "secure-store" | "shell" | "store" | "ui";

/** Optional manifest runtime block. Omit it for the legacy main.ts runtime. */
export interface WasmComponentRuntime {
  kind: "wasm-component";
  abi: "copper.component/1";
  /** Package-local artifact; it must be named `<manifest id>.wasm`. */
  artifact: `${string}.wasm`;
}

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
}

export interface ShellResult {
  code: number;
  stdout: string;
  stderr: string;
}

export type UiMarkup =
  | {
      type: "list";
      title?: string;
      items: Array<Record<string, unknown>>;
      onSelect?: string;
    }
  | {
      type: "form";
      title?: string;
      fields: Array<Record<string, unknown>>;
      onSubmit?: string;
    }
  | {
      type: "detail";
      title?: string;
      content: Record<string, unknown>;
    }
  | {
      type: "toast";
      message: string;
    };

export interface KeyCombo {
  /** Canonical form: ordered modifiers joined by `+`, e.g. `"ctrl+alt+f12"` */
  combo: string;
  /** Human-readable label, e.g. `"Ctrl + Alt + F12"` */
  label: string;
}

export interface Api {
  fs: {
    list(path: string): Promise<FileEntry[]>;
    move(src: string, dst: string): Promise<void>;
    delete(path: string): Promise<void>;
  };
  shell: {
    run(cmd: string, args: string[]): Promise<ShellResult>;
    which(binary: string): Promise<string | null>;
  };
  ui: {
    show(markup: UiMarkup): Promise<void>;
    update(state: Record<string, unknown>): Promise<void>;
  };
  secureStore: {
    /** Retrieves a secret from the OS keychain. Returns `null` if not found. Requires `"secure-store"` permission. */
    get(service: string, key: string): Promise<string | null>;
    /** Stores a secret in the OS keychain (Credential Manager / SecretService / Keychain). Requires `"secure-store"` permission. */
    set(service: string, key: string, value: string): Promise<void>;
    /** Removes a secret from the OS keychain. No-op if the entry does not exist. Requires `"secure-store"` permission. */
    delete(service: string, key: string): Promise<void>;
  };
  keyboard: {
    /** Types a string of text at the current cursor position. Requires `"keyboard"` permission. */
    typeText(text: string): Promise<void>;
    /** Sends a single key press and release, e.g. `"f12"`, `"scroll_lock"`. Requires `"keyboard"` permission. */
    sendKey(key: string): Promise<void>;
    /** Sends a key combination, e.g. `"ctrl+c"`, `"ctrl+alt+f12"`. Requires `"keyboard"` permission. */
    sendCombo(combo: string): Promise<void>;
    /** Normalizes a key combo string to canonical form with an ordered modifier list and a human-readable label. */
    normalizeCombo(combo: string): Promise<KeyCombo>;
  };
  notify(message: string): Promise<void>;
  store: {
    get<T = unknown>(key: string): Promise<T | null>;
    set<T = unknown>(key: string, value: T): Promise<void>;
  };
  windows?: {
    display: {
      status(): Promise<{
        taskbarAutoHide: boolean;
        taskbarPinned: boolean;
        resolution: { width: number; height: number; refreshRate: number };
        scale: { currentPercent: number; availablePercentages: number[] };
      }>;
      toggleTaskbarAutoHide(): Promise<{
        taskbarAutoHide: boolean;
        taskbarPinned: boolean;
      }>;
      setTaskbarAutoHide(autoHide: boolean): Promise<{
        applied: boolean;
        taskbarAutoHide: boolean;
        taskbarPinned: boolean;
      }>;
      setResolution(
        width: number,
        height: number,
        refreshRate: number
      ): Promise<{ applied: boolean }>;
      setScale(scalePercent: number): Promise<{ applied: boolean }>;
    };
  };
  tray?: {
    register(spec: {
      id: string;
      title: string;
      tooltip?: string;
    }): Promise<void>;
    update(id: string, patch: { title?: string; tooltip?: string }): Promise<void>;
    unregister(id: string): Promise<void>;
  };
}
