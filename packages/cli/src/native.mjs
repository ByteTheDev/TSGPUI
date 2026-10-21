// Host operations for the browser preview. They mirror native/src/capabilities.rs: the same grants,
// the same path rules and the same limits. The native host is the reference, so keep the two in step.
// Only `tsgpui preview` serves these, and only to its own page (see the token check in preview.mjs).

import fs from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';

export const MAX_FILE_BYTES = 1024 * 1024;
const MAX_LIST_ENTRIES = 10000;
const PROCESS_TIMEOUT_MS = 10000;

// The grants from a bundle's permissions. Relative roots resolve against `base`, the folder the
// bundle was written to, which is where the native host resolves them for `--bundle` runs.
export function createNativeGrants(permissions, base) {
  const resolveRoots = roots => (roots ?? []).map(root => path.resolve(base, root));
  const write = resolveRoots(permissions?.filesystem?.write);
  // Writable roots are created up front, as the native host does, so a fresh project can save a file.
  for (const root of write) {
    try {
      fs.mkdirSync(root, { recursive: true });
    } catch (_error) {
      // Later calls report that the path is outside a granted directory.
    }
  }
  return {
    base: path.resolve(base),
    read: resolveRoots(permissions?.filesystem?.read),
    write,
    processes: [...(permissions?.processes ?? [])],
  };
}

// Runs one operation and resolves with its JSON value. Failures reject with the message the host shows.
export async function performNative(grants, operation, args = []) {
  switch (operation) {
    case 'filesystem.readText': {
      const file = confine(grants.read, textArg(args, 0), { mustExist: true, allowRoot: false });
      return decodeText(readCapped(file));
    }
    case 'filesystem.writeText': {
      const contents = textArg(args, 1);
      checkSize(Buffer.byteLength(contents));
      fs.writeFileSync(confine(grants.write, textArg(args, 0), { mustExist: false, allowRoot: false }), contents);
      return null;
    }
    case 'filesystem.readBytes': {
      const file = confine(grants.read, textArg(args, 0), { mustExist: true, allowRoot: false });
      return Array.from(readCapped(file));
    }
    case 'filesystem.writeBytes': {
      const bytes = byteArrayArg(args, 1);
      checkSize(bytes.length);
      fs.writeFileSync(confine(grants.write, textArg(args, 0), { mustExist: false, allowRoot: false }), Buffer.from(bytes));
      return null;
    }
    case 'filesystem.list': {
      return listDirectory(confine(grants.read, textArg(args, 0), { mustExist: true, allowRoot: true }));
    }
    case 'filesystem.stat': {
      const target = confine(grants.read, textArg(args, 0), { mustExist: true, allowRoot: true });
      const info = fs.statSync(target);
      return { kind: kindOf(info), size: info.size, modified: Math.trunc(info.mtimeMs) };
    }
    case 'filesystem.mkdir': {
      fs.mkdirSync(confine(grants.write, textArg(args, 0), { mustExist: false, allowRoot: false }));
      return null;
    }
    case 'filesystem.remove': {
      // Non-recursive, the same as the host: a folder must be empty.
      const target = confine(grants.write, textArg(args, 0), { mustExist: true, allowRoot: false });
      if (fs.statSync(target).isDirectory()) fs.rmdirSync(target);
      else fs.unlinkSync(target);
      return null;
    }
    case 'filesystem.rename': {
      const from = confine(grants.write, textArg(args, 0), { mustExist: true, allowRoot: false });
      const to = confine(grants.write, textArg(args, 1), { mustExist: false, allowRoot: false });
      fs.renameSync(from, to);
      return null;
    }
    case 'process.spawn': {
      const command = textArg(args, 0);
      if (!grants.processes.includes(command)) {
        throw new Error(`Process '${command}' is not listed in permissions.processes`);
      }
      return runProcess(command, processArgs(args[1]), grants.base);
    }
    default:
      throw new Error(`Native operation '${operation}' is not available in the browser preview`);
  }
}

// Resolves `relative` under one of `roots`, with the same rules as the host. The path must name
// something inside a root. Symlinks are resolved first, so a link cannot lead out of the root.
function confine(roots, relative, { mustExist, allowRoot }) {
  const segments = relative.split(/[\\/]+/).filter(part => part !== '' && part !== '.');
  const absolute = path.isAbsolute(relative) || path.win32.isAbsolute(relative) || /^[A-Za-z]:/.test(relative);
  if (absolute || segments.includes('..')) {
    throw new Error(`Path '${relative}' must be relative to a granted directory and must not contain '..'`);
  }
  if (segments.length === 0 && !allowRoot) {
    throw new Error(`Path '${relative}' must name a file or folder inside a granted directory`);
  }
  for (const root of roots) {
    let rootReal;
    try {
      rootReal = fs.realpathSync(root);
    } catch (_error) {
      continue;
    }
    const inside = real => real === rootReal || real.startsWith(rootReal + path.sep);
    const candidate = path.join(rootReal, ...segments);
    const real = realpathOrNull(candidate);
    if (real !== null) {
      if (inside(real) && (real !== rootReal || allowRoot)) return real;
      continue;
    }
    // The target does not exist yet. Its parent must exist inside the root, and nothing (not even a
    // dangling symlink) may already sit at the new name.
    const parent = realpathOrNull(path.dirname(candidate));
    if (!mustExist && parent !== null && inside(parent) && !exists(candidate)) {
      return candidate;
    }
  }
  throw new Error(`'${relative}' is not inside a directory granted in permissions.filesystem`);
}

function realpathOrNull(target) {
  try {
    return fs.realpathSync(target);
  } catch (_error) {
    return null;
  }
}

function exists(target) {
  try {
    fs.lstatSync(target);
    return true;
  } catch (_error) {
    return false;
  }
}

function readCapped(file) {
  const info = fs.statSync(file);
  if (!info.isFile()) throw new Error('Path is not a file');
  checkSize(info.size);
  return fs.readFileSync(file);
}

function decodeText(bytes) {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch (_error) {
    throw new Error('File is not valid UTF-8 text');
  }
}

function listDirectory(directory) {
  const entries = fs.readdirSync(directory);
  if (entries.length > MAX_LIST_ENTRIES) throw new Error(`Directory has more than ${MAX_LIST_ENTRIES} entries`);
  return entries
    .map(name => {
      const info = fs.lstatSync(path.join(directory, name));
      return { name, kind: kindOf(info), size: info.size };
    })
    .sort((left, right) => (left.name < right.name ? -1 : left.name > right.name ? 1 : 0));
}

function kindOf(info) {
  if (info.isDirectory()) return 'directory';
  if (info.isFile()) return 'file';
  return 'other';
}

function textArg(args, index) {
  if (typeof args[index] !== 'string') throw new Error(`Argument ${index + 1} must be a string`);
  return args[index];
}

function byteArrayArg(args, index) {
  if (!Array.isArray(args[index])) throw new Error(`Argument ${index + 1} must be an array of bytes`);
  return args[index].map(byte => {
    if (!Number.isInteger(byte) || byte < 0 || byte > 255) throw new Error('Byte values must be 0 to 255');
    return byte;
  });
}

function processArgs(value) {
  if (value === undefined || value === null) return [];
  if (!Array.isArray(value)) throw new Error('Process arguments must be an array of strings');
  if (!value.every(item => typeof item === 'string')) throw new Error('Process arguments must be strings');
  return value;
}

function checkSize(length) {
  if (length > MAX_FILE_BYTES) throw new Error('File exceeds the 1 MiB limit');
}

function runProcess(command, args, cwd) {
  if (/[\\/]/.test(command)) {
    throw new Error('Process names in permissions.processes must not contain path separators');
  }
  return new Promise((resolve, reject) => {
    execFile(
      command,
      args,
      { cwd, timeout: PROCESS_TIMEOUT_MS, maxBuffer: MAX_FILE_BYTES, windowsHide: true, encoding: 'utf8', shell: false },
      (error, stdout, stderr) => {
        if (error && error.killed) {
          reject(new Error(`'${command}' did not finish within ${PROCESS_TIMEOUT_MS / 1000} seconds`));
        } else if (error && error.code === 'ENOENT') {
          reject(new Error(`Unable to start '${command}': ${error.message}`));
        } else if (error && error.code === 'EINVAL') {
          // Node does not start .cmd and .bat files without a shell. The native host can run them.
          reject(new Error(`Unable to start '${command}': batch files run only in the native host`));
        } else {
          // A non-zero exit is a result, not a failure, so `code` carries it.
          const code = error ? (typeof error.code === 'number' ? error.code : null) : 0;
          resolve({ code, stdout: stdout ?? '', stderr: stderr ?? '' });
        }
      },
    );
  });
}
