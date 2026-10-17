import { copyFile, appendFile, rename, rm, chmod, readFile, stat, open } from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';

export const MAGIC = Buffer.from('TSGPUI01','ascii');
export const MAX_PAYLOAD = 16 * 1024 * 1024;

/** Appends [UTF8 JSON][little-endian u64 length][8-byte magic] to a prebuilt native host. */
export async function packageExecutable(hostFile,bundleFile,outputFile) {
  const source = path.resolve(hostFile);
  const output = path.resolve(outputFile);
  if (source === output) throw new Error('Refusing to overwrite the native host binary');
  const payload = await readFile(bundleFile);
  if (!payload.length || payload.length > MAX_PAYLOAD) throw new Error('Invalid bundle size');
  const document = JSON.parse(payload.toString('utf8'));
  if (document.format !== 'tsgpui-1') throw new Error('Incorrect bundle format');
  const hostStat = await stat(source);
  if (!hostStat.isFile() || hostStat.size < 1024) throw new Error('Native host missing or too small');
  const targetIsExe = output.toLowerCase().endsWith('.exe');
  const file = await open(source, 'r');
  const magic = Buffer.alloc(4);
  try { await file.read(magic,0,4,0); } finally { await file.close(); }
  if (targetIsExe && !(magic[0]===0x4d && magic[1]===0x5a)) throw new Error('Windows .exe requires a PE (MZ) host');
  if (!targetIsExe && !(magic[0]===0x7f && magic[1]===0x45 && magic[2]===0x4c && magic[3]===0x46)) {
    throw new Error('Non-Windows builds currently require an ELF host');
  }
  const trailer = Buffer.alloc(16);
  trailer.writeBigUInt64LE(BigInt(payload.length),0);
  MAGIC.copy(trailer,8);
  const temporary = `${output}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  try {
    await copyFile(source,temporary);
    await appendFile(temporary,payload);
    await appendFile(temporary,trailer);
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
