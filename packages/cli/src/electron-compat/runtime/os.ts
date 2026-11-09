import { host } from './host';
export let info: any = {name: '', version: '', platform: '', paths: {}, roots: []};
export const infoReady = host('app.info').then(value => { info = value || info; return info; }, () => info);
export function platform() { const p = info.platform || 'unknown'; return p === 'windows' ? 'win32' : p === 'macos' ? 'darwin' : p; }
export function arch() { return info.arch || 'unknown'; }
export function type() { const p = platform(); return p === 'win32' ? 'Windows_NT' : p === 'darwin' ? 'Darwin' : p === 'linux' ? 'Linux' : p; }
export function homedir() { return info.paths?.home || ''; }
export function tmpdir() { return info.paths?.temp || ''; }
export function hostname() { return info.hostname || ''; }
export const EOL = '\n';
export default {platform, arch, type, homedir, tmpdir, hostname, EOL};
