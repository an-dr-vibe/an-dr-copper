export type Permission = "fs" | "shell" | "network" | "store" | "ui";

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
}
