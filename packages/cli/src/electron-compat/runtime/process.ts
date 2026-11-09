import { info, infoReady } from './os';
export const env: Record<string, string> = {};
export const argv: string[] = [];
export const versions: Record<string, string> = {};
export function cwd() { return info.paths?.appFolder || '/'; }
export function nextTick(fn: Function, ...args: any[]) { Promise.resolve().then(() => fn(...args)); }
export function exit(_code?: number): never { throw new Error('process.exit is not supported'); }
export function on(event: string, _fn: Function): never { throw new Error(`process.on(${JSON.stringify(event)}) is not supported`); }
// The host reports the Rust std::env::consts::OS name ("windows"/"macos"/"linux");
// Node/Electron process.platform uses "win32"/"darwin"/"linux" instead.
const nodePlatform = (value: string) => value === 'windows' ? 'win32' : value === 'macos' ? 'darwin' : value || 'unknown';
export const processShim = { env, argv, versions, cwd, nextTick, exit, on, get platform() { return nodePlatform(info.platform); }, get arch() { return info.arch || 'unknown'; } };
export let platform = nodePlatform(info.platform);
export let arch = info.arch || 'unknown';
infoReady.then(value => { platform = nodePlatform(value.platform); arch = value.arch || 'unknown'; if (value.env) Object.assign(env, value.env); });
export default processShim;
