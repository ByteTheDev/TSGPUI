import {readFile, writeFile} from 'node:fs/promises';
import {createHash, createPrivateKey, sign} from 'node:crypto';
import {spawn, spawnSync} from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';

export async function writeManifest(files, destination, keyPath) {
  const artifacts = {};
  for (const file of files) artifacts[path.basename(file)] = createHash('sha256').update(await readFile(file)).digest('hex');
  const data = Buffer.from(JSON.stringify({algorithm:'sha256', artifacts}, null, 2) + '\n');
  await writeFile(destination, data);
  if (keyPath) {
    const key = createPrivateKey(await readFile(keyPath));
    if (key.asymmetricKeyType !== 'ed25519') throw new Error('Manifest signing key must be Ed25519');
    await writeFile(destination + '.sig', sign(null, data, key).toString('base64') + '\n');
  }
  return destination;
}

// Locate signtool.exe on Windows. Returns an absolute path or null.
export function findSigntool() {
  if (process.platform !== 'win32') return null;
  try {
    const probe = spawnSync('signtool', ['/?'], {stdio: 'ignore', windowsHide: true});
    if (probe.status === 0 || probe.status === 1) return 'signtool';
  } catch {}
  const roots = [
    'C:\\Program Files (x86)\\Windows Kits\\10\\bin',
    'C:\\Program Files\\Windows Kits\\10\\bin',
  ];
  for (const root of roots) {
    if (!fs.existsSync(root)) continue;
    let versions = [];
    try { versions = fs.readdirSync(root).sort().reverse(); } catch {}
    for (const v of versions) {
      const candidate = path.join(root, v, 'x64', 'signtool.exe');
      if (fs.existsSync(candidate)) return candidate;
    }
  }
  return null;
}

// Locate codesign on macOS.
export function findCodesign() {
  if (process.platform !== 'darwin') return null;
  try {
    const probe = spawnSync('codesign', ['--version'], {stdio: 'ignore'});
    if (probe.status === 0) return 'codesign';
  } catch {}
  return null;
}

// Locate xcrun notarytool on macOS.
export function findNotarytool() {
  if (process.platform !== 'darwin') return null;
  try {
    const probe = spawnSync('xcrun', ['notarytool', '--version'], {stdio: 'ignore'});
    if (probe.status === 0) return 'xcrun notarytool';
  } catch {}
  return null;
}

// Locate gpg on Linux.
export function findGpg() {
  if (process.platform !== 'linux') return null;
  try {
    const probe = spawnSync('gpg', ['--version'], {stdio: 'ignore'});
    if (probe.status === 0) return 'gpg';
  } catch {}
  return null;
}

// Resolve the signing command to run. Returns a command string or null (unsigned).
// Precedence: TSGPU_SIGN_COMMAND, then platform-specific env shortcuts.
export function resolveSignCommand() {
  if (process.env.TSGPU_SIGN_COMMAND) return process.env.TSGPU_SIGN_COMMAND;
  if (process.platform === 'win32' && process.env.TSGPU_CERT_FILE) {
    const tool = findSigntool();
    if (!tool) return null;
    const cert = process.env.TSGPU_CERT_FILE;
    const password = process.env.TSGPU_CERT_PASSWORD || '';
    const pw = password ? ` /p "${password}"` : '';
    return `"${tool}" sign /f "${cert}"${pw} /tr http://timestamp.digicert.com /td sha256 /fd sha256`;
  }
  if (process.platform === 'darwin' && process.env.TSGPU_CODESIGN_IDENTITY) {
    const identity = process.env.TSGPU_CODESIGN_IDENTITY;
    return `codesign --sign "${identity}" --timestamp --options runtime`;
  }
  if (process.platform === 'linux' && process.env.TSGPU_GPG_KEY) {
    const key = process.env.TSGPU_GPG_KEY;
    return `gpg --local-user "${key}" --detach-sign --armor`;
  }
  return null;
}

// Notarize a macOS app/binary using xcrun notarytool. Requires APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD.
// Returns true on success, false if skipped, throws on failure.
export async function notarizeArtifact(file) {
  if (process.platform !== 'darwin') return false;
  const appleId = process.env.APPLE_ID;
  const teamId = process.env.APPLE_TEAM_ID;
  const appPassword = process.env.APPLE_APP_PASSWORD;
  if (!appleId || !teamId || !appPassword) {
    console.log('Notarization skipped: set APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD to notarize.');
    return false;
  }
  const tool = findNotarytool();
  if (!tool) { console.log('Notarization skipped: xcrun notarytool not found.'); return false; }
  // Submit and wait.
  const submit = spawnSync('xcrun', ['notarytool', 'submit', path.resolve(file), '--apple-id', appleId, '--team-id', teamId, '--password', appPassword, '--wait'], {stdio: 'inherit'});
  if (submit.status !== 0) throw new Error('Notarization submission failed.');
  // Staple (best-effort; binaries may not staple, apps do).
  try { spawnSync('xcrun', ['stapler', 'staple', path.resolve(file)], {stdio: 'ignore'}); } catch {}
  console.log(`Notarized: ${path.resolve(file)}`);
  return true;
}

export async function signArtifact(file, command = resolveSignCommand()) {
  if (!command) { console.log('Artifact is unsigned (set TSGPU_SIGN_COMMAND, or TSGPU_CERT_FILE on Windows / TSGPU_CODESIGN_IDENTITY on macOS).'); return false; }
  const child = process.platform === 'win32'
    ? spawn('cmd.exe', ['/d', '/s', '/c', `${command} "%TSGPU_SIGN_FILE%"`], {env:{...process.env, TSGPU_SIGN_FILE:path.resolve(file)}, stdio:'ignore', windowsHide:true})
    : spawn('/bin/sh', ['-c', `${command} "$1"`, 'tsgpu-sign', path.resolve(file)], {stdio:'ignore'});
  const code = await new Promise((resolve, reject) => {child.once('error', reject); child.once('close', resolve);});
  if (code !== 0) throw new Error('Signing command failed.');
  return true;
}
