import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, writeFile, readFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {generateKeyPairSync, verify} from 'node:crypto';
import {writeManifest, signArtifact} from '../src/signing.mjs';
import {nsisScript} from '../src/installer/nsis.mjs';

test('installer script includes installation and runtime handling', () => {
  const s = nsisScript({name:'Example',version:'1.0.0',executable:'app.exe',output:'setup.exe'});
  for (const part of ['Example 1.0.0', '$PROGRAMFILES', 'CreateShortcut', 'WriteUninstaller', 'F3017226', 'LinkId=2124703']) assert.ok(s.includes(part));
});
test('unsigned artifact is reported', async () => {
  const old = console.log; let message = ''; console.log = x => {message = x;};
  try { assert.equal(await signArtifact('a', ''), false); assert.match(message, /unsigned/); } finally {console.log = old;}
});
test('manifest hashes bytes and signs exact contents', async () => {
  const dir = await mkdtemp(path.join(tmpdir(),'tsgpui-sign-'));
  try {
    const file = path.join(dir,'a.tgp'), manifest = path.join(dir,'manifest.json'), keyfile = path.join(dir,'key.pem');
    await writeFile(file,'abc');
    const {privateKey, publicKey} = generateKeyPairSync('ed25519');
    await writeFile(keyfile, privateKey.export({type:'pkcs8',format:'pem'}));
    await writeManifest([file],manifest,keyfile);
    const data = await readFile(manifest);
    assert.equal(JSON.parse(data).artifacts['a.tgp'],'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
    assert.ok(verify(null,data,publicKey,Buffer.from((await readFile(manifest+'.sig','utf8')).trim(),'base64')));
  } finally {await rm(dir,{recursive:true,force:true});}
});
