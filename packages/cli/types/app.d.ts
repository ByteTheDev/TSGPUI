/** A TypeScript backend runs in the native host's embedded QuickJS engine.
 * It does NOT have Node.js, Bun, DOM, network or filesystem globals.
 */
export interface BackendAPI<State extends Record<string, unknown>> {
  get<K extends keyof State>(key: K): State[K];
  set<K extends keyof State>(key: K, value: State[K]): void;
  update<K extends keyof State>(key: K, change: (previous: State[K]) => State[K]): void;
  /** Stable API surface for explicit host capabilities. */
  native: NativeBridge;
}

export interface NativeBridge {
  window: {
    minimize(): Promise<void>;
    maximize(): Promise<void>;
    restore(): Promise<void>;
    close(): Promise<void>;
    setTitle(title: string): Promise<void>;
  };
  filesystem: {
    readText(path: string): Promise<string>;
    writeText(path: string, contents: string): Promise<void>;
    readBytes(path: string): Promise<Uint8Array>;
    writeBytes(path: string, contents: Uint8Array): Promise<void>;
  };
  process: {
    spawn(command: string, args?: string[]): Promise<{ code: number | null; stdout: string; stderr: string }>;
  };
  notifications: {
    show(options: { title: string; body?: string }): Promise<void>;
  };
  ipc: {
    send<T>(channel: string, payload: T): Promise<void>;
    on<T>(channel: string, listener: (payload: T) => void): () => void;
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
