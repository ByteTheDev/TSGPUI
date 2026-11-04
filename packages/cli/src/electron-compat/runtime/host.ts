export const compatWarnings: string[] = [];
export function host(op: string, args: any = {}): Promise<any> {
  const g: any = globalThis;
  const fn = g.__tsgpui_host || g.window?.tsgpui?.invoke;
  if (!fn) return Promise.reject(new Error(`Host operation unavailable: ${op}`));
  try { return Promise.resolve(fn(op, args)); } catch (error) { return Promise.reject(error); }
}
const handlers = new Map<string, Function>();
const listeners = new Map<string, Function[]>();
export function handle(channel: string, fn: Function) { if (handlers.has(channel)) throw new Error(`Handler already registered: ${channel}`); handlers.set(channel, fn); }
export function removeHandler(channel: string) { handlers.delete(channel); }
export function listen(channel: string, fn: Function) { listeners.set(channel, [...(listeners.get(channel) || []), fn]); }
export function unlisten(channel: string, fn: Function) { listeners.set(channel, (listeners.get(channel) || []).filter(f => f !== fn && (f as any).listener !== fn)); }
const g: any = globalThis;
g.__tsgpui_ipc_dispatch = async (kind: string, channel: string, payload: any) => {
  const event = { sender: { send: (c: string, p: any) => g.__tsgpui_emit ? g.__tsgpui_emit(c, p) : g.window?.__tsgpui_receive?.(c, p) } };
  if (kind === 'invoke') { const fn = handlers.get(channel); if (!fn) throw new Error(`No handler registered for ${channel}`); return fn(event, payload); }
  for (const fn of [...(listeners.get(channel) || [])]) fn(event, payload);
};

