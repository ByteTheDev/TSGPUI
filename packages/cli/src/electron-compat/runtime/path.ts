export const sep = '/', delimiter = ':';
export function normalize(path: string): string {
  if (!path) return '.';
  const absolute = path.startsWith('/'), trailing = path.endsWith('/');
  const parts: string[] = [];
  for (const part of path.split('/')) { if (!part || part === '.') continue; if (part === '..') { if (parts.length && parts[parts.length - 1] !== '..') parts.pop(); else if (!absolute) parts.push('..'); } else parts.push(part); }
  let result = (absolute ? '/' : '') + parts.join('/');
  if (!result) result = absolute ? '/' : '.';
  if (trailing && result !== '/') result += '/';
  return result;
}
export function join(...paths: string[]) { return normalize(paths.filter(Boolean).join('/')); }
export function resolve(...paths: string[]) { let result = ''; for (let i = paths.length - 1; i >= -1; i--) { const p = i < 0 ? '/' : paths[i]; if (!p) continue; result = p + '/' + result; if (p.startsWith('/')) break; } return normalize(result).replace(/\/$/, '') || '/'; }
export function isAbsolute(p: string) { return p.startsWith('/'); }
export function dirname(p: string) { if (!p) return '.'; const s = p.replace(/\/+$/, ''); const i = s.lastIndexOf('/'); return i < 0 ? (p.startsWith('/') ? '/' : '.') : i === 0 ? '/' : s.slice(0, i); }
export function basename(p: string, suffix?: string) { const s = p.replace(/\/+$/, ''); let b = s.slice(s.lastIndexOf('/') + 1); if (suffix && b.endsWith(suffix)) b = b.slice(0, -suffix.length); return b; }
export function extname(p: string) { const b = basename(p), i = b.lastIndexOf('.'); return i <= 0 || (i === 1 && b === '..') ? '' : b.slice(i); }
export function relative(from: string, to: string) { const a = resolve(from).split('/').filter(Boolean), b = resolve(to).split('/').filter(Boolean); let i = 0; while (i < a.length && i < b.length && a[i] === b[i]) i++; return [...a.slice(i).map(() => '..'), ...b.slice(i)].join('/'); }
export function parse(p: string) { const root = isAbsolute(p) ? '/' : '', base = basename(p), ext = extname(base), dir0 = dirname(p); return { root, dir: dir0 === '.' ? '' : dir0, base, ext, name: base.slice(0, base.length - ext.length) }; }
export function format(o: {root?: string, dir?: string, base?: string, name?: string, ext?: string}) { const dir = o.dir || o.root || '', base = o.base || (o.name || '') + (o.ext || ''); return dir ? dir + (dir === o.root ? '' : '/') + base : base; }
export const posix = { sep, delimiter, normalize, join, resolve, isAbsolute, dirname, basename, extname, relative, parse, format };
export default posix;
