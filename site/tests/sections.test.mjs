import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { buildSite } from '../tools/build.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

const outDir = await mkdtemp(path.join(os.tmpdir(), 'sections-'));
await buildSite({
  srcDir: path.join(SITE_ROOT, 'src'),
  vendorDir: path.join(SITE_ROOT, 'vendor', 'allhere'),
  repoRoot: REPO_ROOT,
  wasmDir: path.join(SITE_ROOT, 'public', 'playground'),
  outDir,
});
const html = await readFile(path.join(outDir, 'index.html'), 'utf8');
const cli = JSON.parse(await readFile(path.join(SITE_ROOT, 'src', 'content', 'cli.json'), 'utf8'));
const claims = JSON.parse(await readFile(path.join(SITE_ROOT, 'src', 'claims.json'), 'utf8'));
const ratios = JSON.parse(await readFile(path.join(SITE_ROOT, 'src', 'content', 'ratios.json'), 'utf8'));

/** The markup of one top-level section: from its opening tag to the next top-level section or </main>. */
function section(id) {
  const start = html.search(new RegExp(`<section class="section[^"]*" id="${id}"`));
  assert.notEqual(start, -1, `section #${id} not found`);
  const rest = html.slice(start + 10);
  const next = rest.search(/<section class="section|<\/main>/);
  return html.slice(start, start + 10 + next);
}
const count = (text, pattern) => [...text.matchAll(pattern)].length;

test('sections 1 to 5 each have a Plain English block and a Developer block', () => {
  for (const id of ['top', 'try', 'why', 'lifecycle', 'read']) {
    const markup = section(id);
    assert.ok(markup.includes('class="v-plain"') || markup.includes('v-plain'), `${id}: v-plain`);
    assert.ok(markup.includes('v-dev'), `${id}: v-dev`);
  }
});

test('the hero says plainly that there is no public release', () => {
  assert.ok(section('top').includes('No public Guion release has been published'));
});

test('the hero art shows the real bytes of the compiled hello program', () => {
  const hero = section('top');
  assert.ok(hero.includes('56 52 54 41'), 'first artifact bytes');
  assert.match(hero, /hello\.gub · \d+ bytes/);
});

test('the playground has every control the loader looks for', () => {
  const markup = section('try');
  for (const id of ['pg-run', 'pg-source', 'pg-output', 'pg-status', 'pg-meta', 'pg-sample']) assert.equal(count(markup, new RegExp(`id="${id}"`, 'g')), 1, id);
  assert.ok(markup.includes('<noscript>'));
  assert.ok(markup.includes('class="pg-notice"'));
  assert.ok(markup.includes('Hello, Guion!'), 'static transcript for visitors without JavaScript');
  for (const sample of ['hello', 'option', 'records', 'denied', 'loop', 'mistake']) assert.ok(markup.includes(`value="${sample}"`), sample);
});

test('the playground states its limits in the Developer view', () => {
  const markup = section('try');
  for (const limit of ['65,536', '10,000', '1 MiB']) assert.ok(markup.includes(limit), limit);
  assert.ok(markup.includes('R_HOST_UNSUPPORTED') && markup.includes('R_STEP_LIMIT'));
});

test('no call to action asks the visitor to download or install', () => {
  for (const [, text] of html.matchAll(/<(?:a|button)\b[^>]*>([\s\S]*?)<\/(?:a|button)>/g)) {
    assert.doesNotMatch(text.replace(/<[^>]+>/g, '').trim(), /^(download|install|get started)/i);
  }
});

test('the Developer lifecycle table lists every command the binary has', () => {
  const markup = section('lifecycle');
  for (const command of cli.commands) assert.ok(markup.includes(`<code>guion ${command.name}</code>`), command.name);
  assert.ok(markup.includes(cli.version) || html.includes(cli.version));
});

test('the Plain lifecycle is the six-stop route in order', () => {
  const markup = section('lifecycle');
  const route = markup.slice(markup.indexOf('class="route"'), markup.indexOf('</ol>', markup.indexOf('class="route"')));
  const order = ['new', 'check', 'test', 'run', 'build', 'package'].map((name) => route.indexOf(`<code>guion ${name}</code>`));
  assert.ok(order.every((index) => index >= 0), JSON.stringify(order));
  assert.deepEqual([...order].sort((a, b) => a - b), order);
});

test('Read it shows the three real programs with their highlighted code and real output', () => {
  const markup = section('read');
  for (const [id, printed] of [['hello', 'Hello, Guion!'], ['option', '24'], ['records', '5']]) {
    assert.ok(markup.includes(`id="panel-${id}"`), id);
    assert.ok(markup.includes(printed), `${id} output`);
  }
  assert.ok(markup.includes('<span class="k">fn</span>'));
  assert.match(markup, /\d+ artifact bytes · \d+ steps/);
});

test('every in-page link points at an id that exists', () => {
  const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]));
  for (const [, target] of html.matchAll(/href="#([^"]+)"/g)) assert.ok(ids.has(target), `dangling #${target}`);
});

test('Permissions: the Developer view shows the real inspect report fields', () => {
  const markup = section('permissions');
  for (const field of ['required_effects', 'manifest_grants', 'grant_assessment', 'present_but_target_dynamic', 'static_bytecode_reachability_not_sandbox']) {
    assert.ok(markup.includes(field), field);
  }
});

test('Permissions: the grant ladder names the real flags and keys', () => {
  const markup = section('permissions');
  for (const flag of ['--file-root', '--allow-http-host', '--allow-clock', '[network]', '[clock]']) assert.ok(markup.includes(flag), flag);
  assert.ok(markup.includes('data-pg-load="denied"'), 'link to the refusal example in the playground');
  assert.ok(markup.includes('not an OS-level sandbox') || markup.includes('not a sandbox'));
});

test('every claim appears exactly once, as a full receipt with a mailto', () => {
  for (const claim of claims) {
    assert.equal(count(html, new RegExp(`data-claim="${claim.id}"`, 'g')), 1, claim.id);
  }
  assert.equal(count(html, /mailto:rmichael@rmichaelthomas\.com\?subject=Guion%20claim/g), claims.length);
});

test('Receipts holds the speed, behaviour and reach claims and a chart of every plotted ratio', () => {
  const markup = section('receipts');
  for (const id of ['richards', 'short-source', 'nine-apps', 'server-gate', 'lockfile', 'conformance', 'concurrency-cancel', 'targets']) assert.ok(markup.includes(`data-claim="${id}"`), id);
  assert.ok(markup.includes('class="ratio-chart"'));
  for (const entry of ratios) assert.ok(markup.includes(entry.label), entry.label);
  for (const id of ['f-all', 'f-speed', 'f-behaviour', 'f-reach']) assert.equal(count(markup, new RegExp(`id="${id}"`, 'g')), 1, id);
});

test('Python still wins: the unfavourable results stay on the page, each with its scope', () => {
  const markup = section('python');
  for (const id of ['bridge-bytes', 'bridge-serialization', 'managed-packages', 'feed-digest', 'python-choice']) assert.ok(markup.includes(`data-claim="${id}"`), id);
  assert.ok(markup.includes('1.025×') && markup.includes('slower in 10 of 15 pairs'));
  assert.ok(markup.includes('all 18 choices') && /historical/i.test(markup));
  assert.ok(markup.includes('Free-threaded Python') || html.includes('Free-threaded Python'));
});

test('every evidence link on the page resolves to a published file', () => {
  const links = [...html.matchAll(/href="(evidence\/[^"#?]+)"/g)].map((match) => match[1]);
  assert.ok(links.length > 20);
  for (const link of links) assert.ok(existsSync(path.join(outDir, link)), link);
});

test('every "open in the playground" button names a real example and is hidden until the playground can run', async () => {
  const samples = JSON.parse(await readFile(path.join(SITE_ROOT, 'src', 'playground', 'samples', 'samples.json'), 'utf8')).map((sample) => sample.id);
  const wanted = [...html.matchAll(/data-pg-load="([^"]+)"/g)].map((match) => match[1]);
  assert.ok(wanted.length >= 4, 'expected buttons in Read it and Permissions');
  for (const id of wanted) assert.ok(samples.includes(id), `data-pg-load="${id}" is not an example`);
  assert.match(html, /body:not\(:has\(#pg-run\[data-ready\]\)\)\s*\[data-pg-load\]\s*\{[^}]*display:\s*none/);
});
