import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { buildSite } from '../tools/build.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

const outDir = await mkdtemp(path.join(os.tmpdir(), 'surface-'));
const { report } = await buildSite({
  srcDir: path.join(SITE_ROOT, 'src'),
  vendorDir: path.join(SITE_ROOT, 'vendor', 'allhere'),
  repoRoot: REPO_ROOT,
  wasmDir: path.join(SITE_ROOT, 'public', 'playground'),
  outDir,
});
const read = (name) => readFile(path.join(outDir, name), 'utf8');
const html = await read('index.html');
const claimsText = await readFile(path.join(SITE_ROOT, 'src', 'claims.json'), 'utf8');
const claims = JSON.parse(claimsText);

function section(id) {
  const start = html.search(new RegExp(`<(?:section|footer) class="section[^"]*" id="${id}"`));
  assert.notEqual(start, -1, id);
  const rest = html.slice(start + 10);
  const next = rest.search(/<section class="section|<footer class="section|<\/main>|<\/body>/);
  return html.slice(start, start + 10 + next);
}

test('agents.json carries every claim with its conditions and evidence', async () => {
  const agents = JSON.parse(await read('agents.json'));
  assert.equal(agents.product, 'Guion');
  assert.match(agents.status, /v0\.1\.0-preview/);
  assert.equal(agents.site, 'https://rmichaelthomas.github.io/guion/');
  assert.equal(agents.contact, 'rmichael@rmichaelthomas.com');
  assert.equal(agents.claims.length, claims.length);
  for (const entry of agents.claims) {
    assert.ok(entry.say && entry.holdsWhen && entry.doesntTell, entry.id);
    assert.ok(entry.evidence.length > 0, entry.id);
    for (const url of entry.evidence) assert.match(url, /^evidence\/[^/]+$/, entry.id);
  }
});

test('llms.txt states the status, lists the claims with conditions, and invents no figure', async () => {
  const text = await read('llms.txt');
  assert.match(text, /v0\.1\.0-preview/);
  assert.ok(text.includes('agents.json') && text.includes('rmichael@rmichaelthomas.com'));
  for (const claim of claims) {
    assert.ok(text.includes(claim.title), claim.title);
    assert.ok(text.includes(claim.plain.holdsWhen), `${claim.id} holds-when`);
    assert.ok(text.includes(claim.plain.doesntTell), `${claim.id} doesnt-tell`);
  }
  const allowed = claimsText + (await read('agents.json'));
  for (const [token] of text.matchAll(/\d[\d,]*(?:\.\d+)?/g)) {
    const bare = token.replace(/,+$/, '');
    if (bare.length >= 3 || /[.,]/.test(bare)) assert.ok(allowed.includes(bare) || /^20\d\d$/.test(bare), `figure not in claims: ${bare}`);
  }
});

test('the agents section offers a copyable prompt and, for developers, the file shapes', () => {
  const markup = section('agents');
  assert.ok(markup.includes('<textarea') && markup.includes('readonly'));
  assert.ok(markup.includes('llms.txt') && markup.includes('agents.json'));
  assert.ok(markup.includes('guion_inspection_report') && markup.includes('guion_check_report'));
});

test('the story has exactly the five dated events, with the rename commit in the Developer view', () => {
  const markup = section('story');
  const dates = [...markup.matchAll(/<time datetime="([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(dates, ['2026-10-22', '2026-10-25', '2026-10-27', '2026-11-03', '2026-11-04']);
  assert.ok(markup.includes('Veritas'));
  assert.ok(markup.includes('6a4fab1'));
});

test('the footer carries licence, contact, the one-line credit, links to every public file, and the weight report', () => {
  const markup = section('footer');
  assert.ok(markup.includes('MIT'));
  assert.ok(markup.includes('rmichael@rmichaelthomas.com'));
  assert.equal([...html.matchAll(/Made with AllHereHTML/g)].length, 1);
  for (const link of ['llms.txt', 'agents.json', 'evidence/', 'developer/']) assert.ok(markup.includes(`href="${link}"`), link);
  assert.ok(markup.includes('0 remote requests') || markup.includes('Remote requests'));
  assert.ok(markup.includes(report.remoteRequests === 0 ? '0' : String(report.remoteRequests)));
  assert.match(markup, /\d+(\.\d)? MB/);
});

test('AllHereHTML is not advertised anywhere else on the page', () => {
  const outside = html.replace(section('footer'), '');
  assert.doesNotMatch(outside, /AllHere/);
});
