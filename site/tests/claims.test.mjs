import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { loadClaims, checkClaims, renderClaimCard } from '../tools/claims.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

async function fixtureRepo(files) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'claims-'));
  for (const [name, text] of Object.entries(files)) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), text);
  }
  return root;
}

const claim = (overrides = {}) => ({
  id: 'sample',
  title: 'A sample claim',
  plain: { say: 'It took 27.46 ms.', holdsWhen: 'On one Mac, with CPython 3.14.7.', doesntTell: 'Anything about other machines.' },
  dev: { rows: [['Guion median', '27.46 ms'], ['Python median', '72.92 ms']], command: 'cargo run -p guion-cli -- check x.gu' },
  figures: ['27.46 ms', '72.92 ms'],
  sources: [{ path: 'reports/a.md', needles: ['CPython 3.14.7'] }],
  evidence: ['reports/a.md'],
  ...overrides,
});
const REPORT = '# Report\n\nGuion: **27.46 ms**. Python: 72.92 ms (CPython 3.14.7, one Mac).\n';

test('a claim whose figures and conditions are in its source passes', async () => {
  const root = await fixtureRepo({ 'reports/a.md': REPORT });
  assert.deepEqual(checkClaims([claim()], root), []);
});

test('a changed figure is reported by name', async () => {
  const root = await fixtureRepo({ 'reports/a.md': REPORT });
  const issues = checkClaims([claim({ figures: ['9.99 ms'] })], root);
  assert.deepEqual(issues, [{ id: 'sample', reason: 'figure not found: 9.99 ms' }]);
});

test('a number written in prose but absent from every source is reported', async () => {
  const root = await fixtureRepo({ 'reports/a.md': REPORT });
  const issues = checkClaims([claim({ plain: { ...claim().plain, say: 'It took 31.07 ms.' } })], root);
  assert.deepEqual(issues, [{ id: 'sample', reason: 'number not in any source: 31.07' }]);
});

test('a missing condition is reported', async () => {
  const root = await fixtureRepo({ 'reports/a.md': REPORT });
  const issues = checkClaims([claim({ sources: [{ path: 'reports/a.md', needles: ['Windows 11'] }] })], root);
  assert.deepEqual(issues, [{ id: 'sample', reason: 'needle not found: Windows 11' }]);
});

test('a missing source file is reported', async () => {
  const root = await fixtureRepo({});
  const issues = checkClaims([claim()], root);
  assert.equal(issues[0].reason, 'missing source: reports/a.md');
});

test('loadClaims rejects duplicate ids and names the claim', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'src-'));
  await writeFile(path.join(dir, 'claims.json'), JSON.stringify([claim(), claim()]));
  assert.throws(() => loadClaims(dir), /sample/);
});

test('a claim card has the five parts, the evidence link, and the exact mailto', () => {
  const html = renderClaimCard(claim());
  for (const label of ['Claim', 'Holds when', "Doesn't tell you", 'Check it', "Say it's wrong"]) assert.ok(html.includes(label), label);
  assert.ok(html.includes('href="evidence/a.md"'));
  assert.ok(html.includes('mailto:rmichael@rmichaelthomas.com?subject=Guion%20claim%3A%20A%20sample%20claim'));
  assert.ok(html.includes('<div class="v-dev">'));
  assert.ok(html.includes('cargo run -p guion-cli -- check x.gu'));
});

test('claim text is escaped', () => {
  const html = renderClaimCard(claim({ title: 'A <b>bold</b> claim' }));
  assert.ok(!html.includes('<b>bold</b>'));
});

test('the real claims are all backed by their sources', () => {
  const claims = loadClaims(path.join(SITE_ROOT, 'src'));
  assert.ok(claims.length >= 12);
  assert.deepEqual(checkClaims(claims, REPO_ROOT), []);
});
