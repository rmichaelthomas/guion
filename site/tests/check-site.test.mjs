import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { checkSite } from '../tools/check-site.mjs';
import { SITE_ROOT } from '../tools/lib/paths.mjs';

const sha = (text) => createHash('sha256').update(text).digest('hex');
const LOADER = '<script type="module" src="playground/loader.mjs"></script>';

/** A minimal valid site; each test then breaks one thing. */
async function fixture(overrides = {}) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'check-'));
  const evidenceText = 'report\n';
  const files = {
    '.nojekyll': '',
    'index.html': `<!doctype html><title>x</title><h1 id="top">x</h1><a href="#top">top</a> <a href="evidence/">evidence</a> <a href="developer/">dev</a> <a href="mailto:a@b.c">mail</a>${LOADER}`,
    'developer/index.html': `<!doctype html><title>x</title><a href="../index.html">home</a><script type="module" src="../playground/loader.mjs"></script>`,
    'playground/loader.mjs': 'export {}',
    'playground/guion_vm.wasm': 'wasm',
    'evidence/report.md': evidenceText,
    'evidence/index.html': `<!doctype html><title>e</title><a href="../index.html">home</a><table><tr><td><a href="report.md">report.md</a></td><td><code>${sha(evidenceText)}</code></td></tr></table>`,
    ...overrides,
  };
  for (const [name, text] of Object.entries(files)) {
    if (text === null) continue;
    await mkdir(path.dirname(path.join(dir, name)), { recursive: true });
    await writeFile(path.join(dir, name), text);
  }
  return dir;
}

test('a minimal valid site has no errors', async () => {
  const { errors } = await checkSite(await fixture());
  assert.deepEqual(errors, []);
});

test('R1: a file nothing links to is reported', async () => {
  const { errors } = await checkSite(await fixture({ 'mockup.html': '<!doctype html><title>m</title>' }));
  assert.deepEqual(errors, ['R1 not linked from any page: mockup.html']);
});

test('R2: an anchor with no target is reported', async () => {
  const { errors } = await checkSite(await fixture({ 'index.html': `<!doctype html><h1 id="top">x</h1><a href="#nope">n</a><a href="evidence/">e</a><a href="developer/">d</a>${LOADER}` }));
  assert.deepEqual(errors, ['R2 index.html: #nope has no matching id']);
});

test('R3: absolute paths and remote URLs are reported', async () => {
  const bad = `<!doctype html><h1 id="top">x</h1><a href="evidence/">e</a><a href="developer/">d</a><img src="/x.png"><link rel="stylesheet" href="https://cdn.example/x.css"><style>a{background:url(//x/y.png)}@import "z.css";</style>${LOADER}`;
  const { errors } = await checkSite(await fixture({ 'index.html': bad }));
  assert.deepEqual(errors.filter((e) => e.startsWith('R3')).sort(), [
    'R3 index.html: @import is not allowed',
    'R3 index.html: absolute or remote reference "//x/y.png"',
    'R3 index.html: absolute or remote reference "/x.png"',
    'R3 index.html: absolute or remote reference "https://cdn.example/x.css"',
  ].sort());
});

test('R4: a second script is reported', async () => {
  const { errors } = await checkSite(await fixture({ 'index.html': `<!doctype html><h1 id="top">x</h1><a href="evidence/">e</a><a href="developer/">d</a>${LOADER}<script>alert(1)</script>` }));
  assert.ok(errors.includes('R4 index.html: unexpected script <script>alert(1)</script>'), JSON.stringify(errors));
});

test('R5: an evidence file changed after hashing is reported', async () => {
  const { errors } = await checkSite(await fixture({ 'evidence/report.md': 'tampered\n' }));
  assert.deepEqual(errors, ['R5 evidence/report.md: sha256 does not match evidence/index.html']);
});

test('R5: an evidence file the index does not list is reported', async () => {
  const { errors } = await checkSite(await fixture({ 'evidence/extra.md': 'x', 'evidence/index.html': '<!doctype html><title>e</title><a href="../index.html">home</a><a href="extra.md">extra.md</a>' }));
  assert.ok(errors.includes('R5 evidence/report.md: not listed with a hash in evidence/index.html') || errors.some((e) => e.startsWith('R5')), JSON.stringify(errors));
});

test('R6: an oversized page is reported', async () => {
  const big = `<!doctype html><h1 id="top">x</h1><a href="evidence/">e</a><a href="developer/">d</a>${LOADER}<p>${'x'.repeat(410 * 1024)}</p>`;
  const { errors } = await checkSite(await fixture({ 'index.html': big }));
  assert.ok(errors.some((e) => e.startsWith('R6 index.html is')), JSON.stringify(errors));
});

test('R7: a missing .nojekyll is reported', async () => {
  const { errors } = await checkSite(await fixture({ '.nojekyll': null }));
  assert.ok(errors.includes('R7 missing .nojekyll'));
});

test('the real built site passes every rule', async () => {
  const { errors, report } = await checkSite(path.join(SITE_ROOT, 'public'));
  assert.deepEqual(errors, []);
  assert.ok(report.files > 20);
  assert.ok(await readFile(path.join(SITE_ROOT, 'public', 'index.html'), 'utf8'));
});
