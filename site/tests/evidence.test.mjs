import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { publishEvidence } from '../tools/evidence.mjs';

const sha = (text) => createHash('sha256').update(text).digest('hex');

async function fixture(files) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'ev-'));
  for (const [name, text] of Object.entries(files)) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), text);
  }
  return root;
}
const claim = (id, evidence, command) => ({ id, title: `Claim ${id}`, dev: { rows: [], ...(command ? { command } : {}) }, evidence });

test('evidence is copied with matching hashes and listed in a generated index', async () => {
  const repoRoot = await fixture({ 'r/a.md': 'alpha\n', 'r/b.json': '{"b":1}\n' });
  const outDir = await mkdtemp(path.join(os.tmpdir(), 'out-'));
  const { entries } = await publishEvidence({ repoRoot, claims: [claim('one', ['r/a.md', 'r/b.json'], 'run-it')], outDir });
  assert.deepEqual(entries.map((e) => e.name), ['a.md', 'b.json']);
  assert.equal(entries[0].sha256, sha('alpha\n'));
  assert.equal(await readFile(path.join(outDir, 'a.md'), 'utf8'), 'alpha\n');
  const index = await readFile(path.join(outDir, 'index.html'), 'utf8');
  for (const entry of entries) {
    assert.ok(index.includes(entry.sha256), entry.name);
    assert.ok(index.includes(`href="${entry.name}"`), entry.name);
  }
  assert.ok(index.includes('Claim one'));
  assert.ok(index.includes('run-it'));
  assert.ok(index.includes('href="../index.html"'));
});

test('a file cited by two claims is published once', async () => {
  const repoRoot = await fixture({ 'r/a.md': 'alpha\n' });
  const outDir = await mkdtemp(path.join(os.tmpdir(), 'out-'));
  const { entries } = await publishEvidence({ repoRoot, claims: [claim('one', ['r/a.md']), claim('two', ['r/a.md'])], outDir });
  assert.equal(entries.length, 1);
});

test('two different files with the same name are refused', async () => {
  const repoRoot = await fixture({ 'x/a.md': '1', 'y/a.md': '2' });
  const outDir = await mkdtemp(path.join(os.tmpdir(), 'out-'));
  await assert.rejects(publishEvidence({ repoRoot, claims: [claim('one', ['x/a.md', 'y/a.md'])], outDir }), /same name/);
});

test('a missing evidence file is refused and named', async () => {
  const repoRoot = await fixture({});
  const outDir = await mkdtemp(path.join(os.tmpdir(), 'out-'));
  await assert.rejects(publishEvidence({ repoRoot, claims: [claim('one', ['r/gone.md'])], outDir }), /r\/gone\.md/);
});

test('republishing after the source changed reports the new hash', async () => {
  const repoRoot = await fixture({ 'r/a.md': 'alpha\n' });
  const outDir = await mkdtemp(path.join(os.tmpdir(), 'out-'));
  const first = await publishEvidence({ repoRoot, claims: [claim('one', ['r/a.md'])], outDir });
  await writeFile(path.join(repoRoot, 'r/a.md'), 'beta\n');
  const second = await publishEvidence({ repoRoot, claims: [claim('one', ['r/a.md'])], outDir });
  assert.notEqual(first.entries[0].sha256, second.entries[0].sha256);
});
