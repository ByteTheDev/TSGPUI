import { info, infoReady } from './os';
export const env: Record<string, string> = {};
export const argv: string[] = [];
export const versions: Record<string, string> = {};
export function cwd() { return info.paths?.appFolder || '/'; }
export function nextTick(fn: Function, ...args: any[]) { Promise.resolve().then(() => fn(...args)); }
export function exit(_code?: number): never { throw new Error('process.exit is not supported'); }
export function on(_event: string, _fn: Function) { return processShim; }
export const processShim = { env, argv, versions, cwd, nextTick, exit, on, get platform() { return info.platform || 'unknown'; } };
export let platform = info.platform || 'unknown';
infoReady.then(value => { platform = value.platform || 'unknown'; if (value.env) Object.assign(env, value.env); });
export default processShim;
