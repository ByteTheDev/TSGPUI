import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, rm} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const cli = fileURLToPath(new URL('../bin/rendrix.mjs', import.meta.url));
test('run returns an error when its host exits unsuccessfully', async t => {
  const tmp = await mkdtemp(path.join(os.tmpdir(), 'rendrix-run-'));
  t.after(() => rm(tmp, {recursive: true, force: true}));
  const project = path.join(tmp, 'app');
  const init = spawnSync(process.execPath, [cli, 'init', project], {encoding: 'utf8'});
  assert.equal(init.status, 0, init.stderr);
  // Node rejects the host's --bundle option, providing a real child-process failure.
  const result = spawnSync(process.execPath, [cli, 'run', '--project', project, '--host', process.execPath], {encoding: 'utf8'});
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /bundle/);
});
