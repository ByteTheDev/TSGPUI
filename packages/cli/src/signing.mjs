import {readFile, writeFile} from 'node:fs/promises';
import {createHash, createPrivateKey, sign} from 'node:crypto';
import {spawn} from 'node:child_process';
import path from 'node:path';

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

export async function signArtifact(file, command = process.env.TSGPUI_SIGN_COMMAND) {
  if (!command) { console.log('Artifact is unsigned (TSGPUI_SIGN_COMMAND is not set).'); return false; }
  // Pass the path as a shell positional parameter, never as part of command text.
  const child = process.platform === 'win32'
    ? spawn('cmd.exe', ['/d', '/s', '/c', `${command} "%TSGPUI_SIGN_FILE%"`], {env:{...process.env, TSGPUI_SIGN_FILE:path.resolve(file)}, stdio:'ignore', windowsHide:true})
    : spawn('/bin/sh', ['-c', `${command} "$1"`, 'tsgpui-sign', path.resolve(file)], {stdio:'ignore'});
  const code = await new Promise((resolve, reject) => {child.once('error', reject); child.once('close', resolve);});
  if (code !== 0) throw new Error('Signing command failed.');
  return true;
}
