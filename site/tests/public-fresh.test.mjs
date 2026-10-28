import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { buildSite } from '../tools/build.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

async function walk(dir, prefix = '') {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) found.push(...await walk(path.join(dir, entry.name), relative));
    else found.push(relative);
  }
  return found.sort();
}

test('the committed site/public is exactly what a fresh build produces', async () => {
  const fresh = await mkdtemp(path.join(os.tmpdir(), 'fresh-'));
  const publicDir = path.join(SITE_ROOT, 'public');
  await buildSite({
    srcDir: path.join(SITE_ROOT, 'src'),
    vendorDir: path.join(SITE_ROOT, 'vendor', 'allhere'),
    repoRoot: REPO_ROOT,
    wasmDir: path.join(publicDir, 'playground'),
    outDir: fresh,
  });
  const [built, committed] = [await walk(fresh), await walk(publicDir)];
  assert.deepEqual(committed, built, 'file lists differ: run node site/tools/build.mjs');
  for (const file of built) {
    assert.deepEqual(await readFile(path.join(publicDir, file)), await readFile(path.join(fresh, file)), `${file} is stale: run node site/tools/build.mjs`);
  }
});
