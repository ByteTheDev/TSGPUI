import { host } from './host';
import { infoReady } from './os';
import * as path from './path';
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function encode(bytes: Uint8Array) { let out = ''; for (let i = 0; i < bytes.length; i += 3) { const n = (bytes[i] << 16) | ((bytes[i+1] || 0) << 8) | (bytes[i+2] || 0); out += alphabet[n >> 18] + alphabet[(n >> 12) & 63] + (i+1 < bytes.length ? alphabet[(n >> 6) & 63] : '=') + (i+2 < bytes.length ? alphabet[n & 63] : '='); } return out; }
function decode(s: string) { const bytes: number[] = []; let value = 0, bits = 0; for (const c of s) { if (c === '=') break; const n = alphabet.indexOf(c); if (n < 0) continue; value = (value << 6) | n; bits += 6; if (bits >= 8) { bits -= 8; bytes.push((value >> bits) & 255); } } return new Uint8Array(bytes); }
async function mapped(input: string, write = false) { await infoReady; const info = await host('app.info'); const norm = (s: string) => path.resolve(s.replace(/\\/g, '/')); const target = norm(input); const roots = (info.roots || []).filter((r: any) => !write || r.access === 'write').map((r: any) => ({...r, normalized: norm(r.path)})).sort((a: any,b: any) => b.normalized.length-a.normalized.length); const root = roots.find((r: any) => target === r.normalized || (!r.exact && target.startsWith(r.normalized === '/' ? '/' : r.normalized + '/'))); if (!root) { const err: any = new Error(`EACCES: path outside permitted roots: ${input}`); err.code = 'EACCES'; throw err; } return {root: root.name, path: path.relative(root.normalized, target)}; }
function encoding(options: any) { return typeof options === 'string' ? options : options?.encoding; }
async function promise_readFile(p: string, options?: any) { const loc = await mapped(p); if (encoding(options)) return (await host('filesystem.readText', loc)).text; return decode((await host('filesystem.readBytes', loc)).base64); }
async function promise_writeFile(p: string, data: any, _options?: any) { const loc = await mapped(p, true); if (typeof data === 'string') await host('filesystem.writeText', {...loc, text: data}); else await host('filesystem.writeBytes', {...loc, base64: encode(new Uint8Array(data))}); }
async function promise_readdir(p: string, _options?: any) { const result = await host('filesystem.list', await mapped(p)); return result.entries ?? result.names ?? result; }
async function promise_stat(p: string) { const result = await host('filesystem.stat', await mapped(p)); const isDir = result.isDirectory === true || result.kind === 'directory' || result.type === 'directory'; return {...result, isDirectory: () => isDir, isFile: () => !isDir}; }
async function promise_mkdir(p: string, _options?: any) { await host('filesystem.mkdir', await mapped(p, true)); }
async function promise_rm(p: string, _options?: any) { await host('filesystem.remove', await mapped(p, true)); }
const promise_unlink = promise_rm;
async function promise_rename(from: string, to: string) { const a = await mapped(from, true), b = await mapped(to, true); if (a.root !== b.root) throw new Error('EXDEV: cross-root rename is not supported'); await host('filesystem.rename', {root: a.root, from: a.path, to: b.path}); }
async function promise_access(p: string, mode?: number) { await host('filesystem.stat', await mapped(p, !!(mode && (mode & 2)))); }
export const promises = {readFile: promise_readFile, writeFile: promise_writeFile, readdir: promise_readdir, stat: promise_stat, mkdir: promise_mkdir, rm: promise_rm, unlink: promise_unlink, rename: promise_rename, access: promise_access};
function dual(fn: Function) { return (...args: any[]) => { const cb = args[args.length - 1]; if (typeof cb === 'function') { args.pop(); fn(...args).then((value: any) => cb(null, value), (err: any) => cb(err)); return; } return fn(...args); }; }
export const readFile = dual(promise_readFile), writeFile = dual(promise_writeFile), readdir = dual(promise_readdir), stat = dual(promise_stat), mkdir = dual(promise_mkdir), rm = dual(promise_rm), unlink = dual(promise_unlink), rename = dual(promise_rename), access = dual(promise_access);
function unsupported(): never { const e: any = new Error('ENOTSUP: synchronous fs operations are not supported; use fs.promises'); e.code = 'ENOTSUP'; throw e; }
export const readFileSync = unsupported, writeFileSync = unsupported, readdirSync = unsupported, statSync = unsupported, mkdirSync = unsupported, rmSync = unsupported, unlinkSync = unsupported, renameSync = unsupported, accessSync = unsupported, existsSync = unsupported;
const fs = {promises, readFile, writeFile, readdir, stat, mkdir, rm, unlink, rename, access, readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, rmSync, unlinkSync, renameSync, accessSync, existsSync};
export default fs;
