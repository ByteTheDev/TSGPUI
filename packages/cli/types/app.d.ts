/** A TypeScript backend runs in the native host's embedded QuickJS engine.
 * It does NOT have Node.js, Bun, DOM, network or filesystem globals.
 */
export interface BackendAPI<State extends Record<string, unknown>> {
  get<K extends keyof State>(key: K): State[K];
  set<K extends keyof State>(key: K, value: State[K]): void;
  update<K extends keyof State>(key: K, change: (previous: State[K]) => State[K]): void;
}
export interface AppDefinition<State extends Record<string, unknown>> {
  state: State;
  actions: Record<string, (api: BackendAPI<State>) => void>;
}
