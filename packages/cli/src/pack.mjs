import { copyFile, appendFile, rename, rm, chmod, readFile, stat, open, mkdir, writeFile, lstat } from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';

export const MAGIC = Buffer.from('TSGPUI01','ascii');
export const MAX_PAYLOAD = 16 * 1024 * 1024;

function isMachO(header) {
  if (header.length < 4) return false;
  const magic = header.readUInt32BE(0);
  return [
    0xfeedface, 0xcefaedfe, // 32-bit thin, either byte order
    0xfeedfacf, 0xcffaedfe, // 64-bit thin, either byte order
    0xcafebabe, 0xbebafeca, // universal/fat
    0xcafebabf, 0xbfbafeca  // 64-bit universal/fat
  ].includes(magic);
}

async function readHeader(filePath) {
  const file = await open(filePath, 'r');
  const header = Buffer.alloc(4);
  try { await file.read(header,0,4,0); } finally { await file.close(); }
  return header;
}

function plistEscape(value) {
  return String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}

async function validateBundle(bundleFile) {
  const payload = await readFile(bundleFile);
  if (!payload.length || payload.length > MAX_PAYLOAD) throw new Error('Invalid bundle size');
  const document = JSON.parse(payload.toString('utf8'));
  if (document.format !== 'tsgpui-1') throw new Error('Incorrect bundle format');
  return {payload, document};
}

function appendOverlay(target, payload) {
  const trailer = Buffer.alloc(16);
  trailer.writeBigUInt64LE(BigInt(payload.length),0);
  MAGIC.copy(trailer,8);
  return appendFile(target,payload).then(() => appendFile(target,trailer));
}

/** Builds a conventional unsigned macOS .app bundle around a matching Mach-O host. */
export async function packageMacApp(hostFile,bundleFile,outputDirectory) {
  const source = path.resolve(hostFile);
  const output = path.resolve(outputDirectory);
  if (!output.toLowerCase().endsWith('.app')) throw new Error('macOS application output must end in .app');
  if (source === output) throw new Error('Refusing to overwrite the native host binary');
  const {payload,document} = await validateBundle(bundleFile);
  if (typeof document.name !== 'string' || !/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(document.name)) throw new Error('macOS app bundle has an invalid app name');
  const hostStat = await stat(source);
  if (!hostStat.isFile() || hostStat.size < 1024 || !isMachO(await readHeader(source))) throw new Error('macOS .app requires a Mach-O host');
  const existing = await lstat(output).catch(() => null);
  if (existing?.isSymbolicLink()) throw new Error('Refusing to replace a symlink at the macOS application output');
  const temporary = `${output}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  const executable = path.join(temporary,'Contents','MacOS',document.name);
  const bundleIdSuffix = document.name.replaceAll('_', '-');
  const plist = `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict><key>CFBundleExecutable</key><string>${plistEscape(document.name)}</string><key>CFBundleIdentifier</key><string>dev.tsgpui.${plistEscape(bundleIdSuffix)}</string><key>CFBundleName</key><string>${plistEscape(document.title || document.name)}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>1.0.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>\n`;
  try {
    await mkdir(path.dirname(executable),{recursive:true});
    await copyFile(source,executable);
    await appendOverlay(executable,payload);
    await chmod(executable,hostStat.mode);
    await mkdir(path.join(temporary,'Contents','Resources'),{recursive:true});
    await writeFile(path.join(temporary,'Contents','Info.plist'),plist);
    if (existing) await rm(output,{recursive:true,force:true});
    await rename(temporary,output);
    return {file:output,bytes:payload.length};
  } catch (err) {
    await rm(temporary,{recursive:true,force:true});
    throw err;
  }
}

/** Appends [UTF8 JSON][little-endian u64 length][8-byte magic] to a prebuilt native host. */
export async function packageExecutable(hostFile,bundleFile,outputFile) {
  const source = path.resolve(hostFile);
  const output = path.resolve(outputFile);
  if (source === output) throw new Error('Refusing to overwrite the native host binary');
  const {payload} = await validateBundle(bundleFile);
  const hostStat = await stat(source);
  if (!hostStat.isFile() || hostStat.size < 1024) throw new Error('Native host missing or too small');
  const targetIsExe = output.toLowerCase().endsWith('.exe');
  const header = await readHeader(source);
  const isPe = header[0]===0x4d && header[1]===0x5a;
  const isElf = header[0]===0x7f && header[1]===0x45 && header[2]===0x4c && header[3]===0x46;
  if (targetIsExe && !isPe) throw new Error('Windows .exe requires a PE (MZ) host');
  if (!targetIsExe && !(isElf || isMachO(header))) {
    throw new Error('Native packaging requires a PE (.exe), ELF (Linux), or Mach-O (macOS) host');
  }
  const temporary = `${output}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  try {
    await copyFile(source,temporary);
    await appendOverlay(temporary,payload);
    await chmod(temporary,hostStat.mode);
    await rename(temporary,output);
    return {file:output,bytes:payload.length};
  } catch (err) {
    await rm(temporary,{force:true});
    throw err;
  }
}
export function extractEmbeddedBundleForTest(bytes) {
  if (!bytes.subarray(bytes.length-8).equals(MAGIC)) throw new Error('Missing TSGPUI trailer');
  const length = Number(bytes.readBigUInt64LE(bytes.length-16));
  if (length <= 0 || length > MAX_PAYLOAD || length > bytes.length-16) throw new Error('Invalid bundle length');
  return JSON.parse(bytes.subarray(bytes.length-16-length,bytes.length-16).toString('utf8'));
}
