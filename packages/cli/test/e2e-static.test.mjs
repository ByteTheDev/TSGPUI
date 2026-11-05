import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
test('Windows e2e script defines assertions and workflow runs it', async () => {
  const script = await readFile(new URL('../../../scripts/e2e-windows.mjs', import.meta.url), 'utf8');
  for (const name of ['exceptions', 'styles', 'responding', 'canceled']) assert.match(script, new RegExp(`\\b${name}:`));
  const workflow = await readFile(new URL('../../../.github/workflows/platform-validation.yml', import.meta.url), 'utf8');
  assert.match(workflow, /scripts\/e2e-windows\.mjs/);
});
