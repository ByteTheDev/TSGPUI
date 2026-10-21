/** A TypeScript backend runs in the native host's embedded QuickJS engine.
 * It does NOT have Node.js, Bun, DOM, network or filesystem globals.
 */
export interface BackendAPI<State extends Record<string, unknown>> {
  get<K extends keyof State>(key: K): State[K];
  set<K extends keyof State>(key: K, value: State[K]): void;
  update<K extends keyof State>(key: K, change: (previous: State[K]) => State[K]): void;
  /** Host operations, each checked against the grants in `tsgpui.config.json`. Calls return
   * promises and reject when the grant is missing or the operation fails. */
  native: NativeBridge;
}

/** Paths are relative to a granted directory: `notes/a.txt` means `<granted root>/notes/a.txt`.
 * They may not contain `..` or be absolute, and symlinks may not lead outside the root.
 * `''` or `'.'` names the granted folder itself, for `list` and `stat` only.
 * Text and bytes are limited to 1 MiB per call. Calls run on the UI thread, so keep them short. */
export interface NativeBridge {
  /** Needs `permissions.window`. The browser preview supports `setTitle` and `close` only. */
  window: {
    minimize(): Promise<void>;
    maximize(): Promise<void>;
    restore(): Promise<void>;
    close(): Promise<void>;
    setTitle(title: string): Promise<void>;
  };
  filesystem: {
    /** Needs `permissions.filesystem.read`. */
    readText(path: string): Promise<string>;
    /** Needs `permissions.filesystem.write`. Creates or overwrites the file. */
    writeText(path: string, contents: string): Promise<void>;
    readBytes(path: string): Promise<Uint8Array>;
    writeBytes(path: string, contents: Uint8Array): Promise<void>;
    /** Needs `read`. Sorted by name. */
    list(path: string): Promise<Array<{ name: string; kind: 'file' | 'directory' | 'other'; size: number }>>;
    /** Needs `read`. `modified` is milliseconds since the Unix epoch. */
    stat(path: string): Promise<{ kind: 'file' | 'directory' | 'other'; size: number; modified: number | null }>;
    /** Needs `write`. Creates one folder; the parent must exist. */
    mkdir(path: string): Promise<void>;
    /** Needs `write`. Non-recursive: a folder must be empty. */
    remove(path: string): Promise<void>;
    /** Needs `write` for both paths. */
    rename(from: string, to: string): Promise<void>;
  };
  process: {
    /** `command` must be listed in `permissions.processes`. Runs without a shell, in the app's folder,
     * with a 10 second limit. Output is capped at 1 MiB per stream. */
    spawn(command: string, args?: string[]): Promise<{ code: number | null; stdout: string; stderr: string }>;
  };
  /** Needs `permissions.notifications`. Shows a desktop notification (a toast on Windows). */
  notifications: {
    show(options: { title: string; body?: string }): Promise<void>;
  };
  /** Needs `permissions.ipc`. Messages go to other local processes, and to this app's own listeners.
   * A child process started with `process.spawn` gets `TSGPUI_IPC_ADDR` and `TSGPUI_IPC_TOKEN`
   * (the wire format is in docs/SUPPORTED_SYNTAX.md, under `api.native`). */
  ipc: {
    send<T>(channel: string, payload: T): Promise<void>;
    /** Listeners run as one task, so a listener that writes state rolls back if it throws. */
    on<T>(channel: string, listener: (payload: T) => void | Promise<void>): () => void;
  };
}
/** Data supplied by native buttons and controls. Fields depend on the event. */
export interface BackendEvent {
  type?: 'click' | 'input' | 'submit' | 'shortcut' | 'confirm' | 'menu' | 'contextmenu' | 'drop';
  value?: string | number | boolean;
  checked?: boolean;
  name?: string;
  key?: string;
  submit?: boolean;
  files?: Array<{ name: string; path: string }>;
}
export interface AppDefinition<State extends Record<string, unknown>> {
  state: State;
  actions: Record<string, (api: BackendAPI<State>, event: Readonly<BackendEvent>) => void | Promise<void>>;
}
