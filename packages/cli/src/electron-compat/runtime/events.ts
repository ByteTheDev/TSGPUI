export class EventEmitter {
  private events = new Map<string | symbol, Function[]>();
  on(name: string | symbol, fn: Function) { this.events.set(name, [...(this.events.get(name) || []), fn]); return this; }
  once(name: string | symbol, fn: Function) { const wrapper = (...args: any[]) => { this.off(name, wrapper); fn.apply(this, args); }; (wrapper as any).listener = fn; return this.on(name, wrapper); }
  off(name: string | symbol, fn: Function) { this.events.set(name, (this.events.get(name) || []).filter(f => f !== fn && (f as any).listener !== fn)); return this; }
  removeListener(name: string | symbol, fn: Function) { return this.off(name, fn); }
  emit(name: string | symbol, ...args: any[]) { const list = [...(this.events.get(name) || [])]; for (const fn of list) fn.apply(this, args); return list.length > 0; }
  removeAllListeners(name?: string | symbol) { if (name === undefined) this.events.clear(); else this.events.delete(name); return this; }
  listenerCount(name: string | symbol) { return (this.events.get(name) || []).length; }
}
export default EventEmitter;
