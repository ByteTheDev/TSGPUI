import { host } from './host';
import { infoReady } from './os';
import * as path from './path';
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function encode(bytes: Uint8Array) { let out = ''; for (let i = 0; i < bytes.length; i += 3) { const n = (bytes[i] << 16) | ((bytes[i+1] || 0) << 8) | (bytes[i+2] || 0); out += alphabet[n >> 18] + alphabet[(n >> 12) & 63] + (i+1 < bytes.length ? alphabet[(n >> 6) & 63] : '=') + (i+2 < bytes.length ? alphabet[n & 63] : '='); } return out; }
function decode(s: string) { const bytes: number[] = []; let value = 0, bits = 0; for (const c of s) { if (c === '=') break; const n = alphabet.indexOf(c); if (n < 0) continue; value = (value << 6) | n; bits += 6; if (bits >= 8) { bits -= 8; bytes.push((value >> bits) & 255); } } return new Uint8Array(bytes); }
async function mapped(input: string, write = false) { await infoReady; const info = await host('app.info'); const norm = (s: string) => path.resolve(s.replace(/\\/g, '/')); const target = norm(input); const roots = (info.roots || []).filter((r: any) => !write || r.access === 'write').map((r: any) => ({...r, normalized: norm(r.path)})).sort((a: any,b: any) => b.normalized.length-a.normalized.length); const root = roots.find((r: any) => target === r.normalized || (!r.exact && target.startsWith(r.normalized === '/' ? '/' : r.normalized + '/'))); if (!root) { const err: any = new Error(`EACCES: path outside permitted roots: ${input}`); err.code = 'EACCES'; throw err; } return {root: root.name, path: path.relative(root.normalized, target)}; }
// Maps an absolute-looking app path onto a `{root, path}` pair the host understands. nativeImage
// uses this to read a file through the same grants as fs, including dialog-selected files.
export async function resolveLocation(input: string, write = false) { return mapped(input, write); }
function encoding(options: any) { return typeof options === 'string' ? options : options?.encoding; }
function checkOptions(api: string, options: any) { const enc = encoding(options); if (enc && !['utf8','utf-8'].includes(String(enc).toLowerCase())) throw new Error(`fs.${api} encoding '${enc}' is not supported`); if (options && typeof options === 'object' && Object.keys(options).some(k => k !== 'encoding')) throw new Error(`fs.${api} options are not supported`); }
async function promise_readFile(p: string, options?: any) { checkOptions('readFile', options); const loc = await mapped(p); if (encoding(options)) return (await host('filesystem.readText', loc)).text; return decode((await host('filesystem.readBytes', loc)).base64); }
async function promise_writeFile(p: string, data: any, options?: any) { checkOptions('writeFile', options); const loc = await mapped(p, true); if (typeof data === 'string') await host('filesystem.writeText', {...loc, text: data}); else if (data instanceof Uint8Array) await host('filesystem.writeBytes', {...loc, base64: encode(data)}); else throw new TypeError('fs.writeFile supports strings and Uint8Array data'); }
// Electron's readdir returns names, or Dirent-like objects with withFileTypes. The hosts list entries as {name, kind, size}.
async function promise_readdir(p: string, options?: any) {
  const result = await host('filesystem.list', await mapped(p));
  // Arrays have an `entries` method, so check for an array before reading the wrapper fields.
  const entries: any[] = Array.isArray(result) ? result : (result.entries ?? result.names ?? []);
  const withTypes = typeof options === 'object' && options !== null && options.withFileTypes === true;
  if (!withTypes) return entries.map((entry: any) => typeof entry === 'string' ? entry : entry.name);
  return entries.map((entry: any) => { const name = typeof entry === 'string' ? entry : entry.name; const isDir = entry.kind === 'directory'; return {name, isDirectory: () => isDir, isFile: () => !isDir}; });
}
// Electron's Stats also carries mtimeMs and mtime; the hosts report `modified` as milliseconds since the epoch.
async function promise_stat(p: string) {
  const result = await host('filesystem.stat', await mapped(p));
  const isDir = result.isDirectory === true || result.kind === 'directory' || result.type === 'directory';
  const mtimeMs = typeof result.modified === 'number' ? result.modified : 0;
  return {...result, mtimeMs, mtime: new Date(mtimeMs), isDirectory: () => isDir, isFile: () => !isDir};
}
async function promise_mkdir(p: string, _options?: any) { await host('filesystem.mkdir', await mapped(p, true)); }
async function promise_rm(p: string, _options?: any) { await host('filesystem.remove', await mapped(p, true)); }
const promise_unlink = promise_rm;
async function promise_rename(from: string, to: string) { const a = await mapped(from, true), b = await mapped(to, true); if (a.root !== b.root) throw new Error('EXDEV: cross-root rename is not supported'); await host('filesystem.rename', {root: a.root, from: a.path, to: b.path}); }
async function promise_access(p: string, mode = 0) { if (mode & ~6) throw new Error('fs.access execute mode is not supported'); await host('filesystem.stat', await mapped(p, !!(mode & 2))); }
async function promise_copyFile(from: string, to: string, flags = 0) { if (flags !== 0) throw new Error('fs.copyFile flags are not supported'); await promise_writeFile(to, await promise_readFile(from)); }
export const constants = {F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1};
export const promises = {readFile: promise_readFile, writeFile: promise_writeFile, readdir: promise_readdir, stat: promise_stat, mkdir: promise_mkdir, rm: promise_rm, unlink: promise_unlink, rename: promise_rename, access: promise_access, copyFile: promise_copyFile, constants};
function dual(fn: Function) { return (...args: any[]) => { const cb = args[args.length - 1]; if (typeof cb === 'function') { args.pop(); fn(...args).then((value: any) => cb(null, value), (err: any) => cb(err)); return; } return fn(...args); }; }
export const readFile = dual(promise_readFile), writeFile = dual(promise_writeFile), readdir = dual(promise_readdir), stat = dual(promise_stat), mkdir = dual(promise_mkdir), rm = dual(promise_rm), unlink = dual(promise_unlink), rename = dual(promise_rename), access = dual(promise_access);
export const copyFile = dual(promise_copyFile);
function unsupported(name: string) { return (..._args: any[]): never => { const e: any = new Error(`ENOTSUP: fs.${name} is not supported; use fs.promises`); e.code = 'ENOTSUP'; throw e; }; }
export const readFileSync = unsupported('readFileSync'), writeFileSync = unsupported('writeFileSync'), readdirSync = unsupported('readdirSync'), statSync = unsupported('statSync'), mkdirSync = unsupported('mkdirSync'), rmSync = unsupported('rmSync'), unlinkSync = unsupported('unlinkSync'), renameSync = unsupported('renameSync'), accessSync = unsupported('accessSync'), existsSync = unsupported('existsSync'), copyFileSync = unsupported('copyFileSync');
const fs = {promises, constants, readFile, writeFile, readdir, stat, mkdir, rm, unlink, rename, access, copyFile, readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, rmSync, unlinkSync, renameSync, accessSync, existsSync, copyFileSync};
export default fs;
