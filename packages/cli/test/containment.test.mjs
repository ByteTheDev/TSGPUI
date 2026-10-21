import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { mkdtemp, writeFile, readFile, rm, mkdir, readdir, symlink } from 'node:fs/promises';
import { buildPackage, readProject } from '../src/compiler.mjs';

const html = '<body><main><h1>Hi</h1></main></body>';
const css = 'body { color:#fff; }';
const backend = `export default { state:{ count: 0 }, actions:{} };`;
const config = { name: 'contained', title: 'Contained', width: 500, height: 400, html: 'src/index.html', css: 'src/styles.css', backend: 'src/backend.ts', outDir: 'dist' };

// Creates a project under tmp/project and an unrelated "outside" folder that symlinks must not reach.
async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), 'tsgpui-contain-'));
  const project = path.join(root, 'project');
  const outside = path.join(root, 'outside');
  await mkdir(path.join(project, 'src'), { recursive: true });
  await mkdir(outside, { recursive: true });
  await writeFile(path.join(project, 'tsgpui.config.json'), JSON.stringify(config));
  await writeFile(path.join(project, 'src/index.html'), html);
  await writeFile(path.join(project, 'src/styles.css'), css);
  await writeFile(path.join(project, 'src/backend.ts'), backend);
  return { root, project, outside };
}

// Symlinks need privileges on some Windows setups; skip rather than fail where they are unavailable.
async function link(target, location, type = 'file') {
  try {
    await symlink(target, location, type);
    return true;
  } catch (error) {
    if (error.code === 'EPERM' || error.code === 'EACCES') return false;
    throw error;
  }
}

test('a valid project builds with a missing output directory', async () => {
  const { root, project } = await fixture();
  try {
    const result = await buildPackage(project);
    assert.equal(path.basename(result.bundleFile), 'contained.tgp');
    assert.ok((await readFile(result.bundleFile, 'utf8')).includes('"tsgpui-1"'));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('an HTML source symlinked outside the project is rejected', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    await writeFile(path.join(outside, 'secret.html'), html);
    await rm(path.join(project, 'src/index.html'));
    if (!await link(path.join(outside, 'secret.html'), path.join(project, 'src/index.html'))) return t.skip('symlinks unavailable');
    await assert.rejects(buildPackage(project), /HTML resolves outside the project/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a backend symlinked outside the project is rejected', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    await writeFile(path.join(outside, 'evil.ts'), backend);
    await rm(path.join(project, 'src/backend.ts'));
    if (!await link(path.join(outside, 'evil.ts'), path.join(project, 'src/backend.ts'))) return t.skip('symlinks unavailable');
    await assert.rejects(readProject(project), /Backend resolves outside the project/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('the config file symlinked outside the project is rejected', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    await writeFile(path.join(outside, 'config.json'), JSON.stringify(config));
    await rm(path.join(project, 'tsgpui.config.json'));
    if (!await link(path.join(outside, 'config.json'), path.join(project, 'tsgpui.config.json'))) return t.skip('symlinks unavailable');
    await assert.rejects(readProject(project), /tsgpui.config.json resolves outside the project/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('an output directory symlinked outside the project is rejected and nothing is written there', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    if (!await link(outside, path.join(project, 'dist'), 'junction')) return t.skip('symlinks unavailable');
    await assert.rejects(buildPackage(project), /Output directory resolves outside the project/);
    assert.deepEqual(await readdir(outside), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a missing output directory whose parent is a symlink outside the project is rejected', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    if (!await link(outside, path.join(project, 'build'), 'junction')) return t.skip('symlinks unavailable');
    await writeFile(path.join(project, 'tsgpui.config.json'), JSON.stringify({ ...config, outDir: 'build/nested' }));
    await assert.rejects(buildPackage(project), /Output directory resolves outside the project/);
    assert.deepEqual(await readdir(outside), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a dangling symlink in a source path is rejected', async (t) => {
  const { root, project } = await fixture();
  try {
    await rm(path.join(project, 'src/styles.css'));
    if (!await link(path.join(root, 'missing.css'), path.join(project, 'src/styles.css'))) return t.skip('symlinks unavailable');
    await assert.rejects(readProject(project), /CSS is a dangling symlink/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('an output directory symlink that stays inside the project is allowed', async (t) => {
  const { root, project } = await fixture();
  try {
    await mkdir(path.join(project, 'real-out'));
    if (!await link(path.join(project, 'real-out'), path.join(project, 'dist'), 'junction')) return t.skip('symlinks unavailable');
    const result = await buildPackage(project);
    assert.ok((await readdir(path.join(project, 'real-out'))).includes('contained.tgp'));
    assert.ok(result.bundleFile);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('an existing output file that is a symlink is not written through', async (t) => {
  const { root, project, outside } = await fixture();
  try {
    await mkdir(path.join(project, 'dist'));
    await writeFile(path.join(outside, 'target.tgp'), 'original');
    if (!await link(path.join(outside, 'target.tgp'), path.join(project, 'dist/contained.tgp'))) return t.skip('symlinks unavailable');
    await assert.rejects(buildPackage(project), /Refusing to write through symlink/);
    assert.equal(await readFile(path.join(outside, 'target.tgp'), 'utf8'), 'original');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
