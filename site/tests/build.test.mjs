import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { buildSite } from '../tools/build.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

const real = {
  srcDir: path.join(SITE_ROOT, 'src'),
  vendorDir: path.join(SITE_ROOT, 'vendor', 'allhere'),
  repoRoot: REPO_ROOT,
  wasmDir: path.join(SITE_ROOT, 'public', 'playground'),
};
const tmp = (prefix) => mkdtemp(path.join(os.tmpdir(), prefix));

async function allFiles(dir, prefix = '') {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const relative = path.posix.join(prefix, entry.name);
    if (entry.isDirectory()) found.push(...await allFiles(path.join(dir, entry.name), relative));
    else found.push(relative);
  }
  return found.sort();
}

const outA = await tmp('site-a-');
const resultA = await buildSite({ ...real, outDir: outA });
const html = await readFile(path.join(outA, 'index.html'), 'utf8');
const devHtml = await readFile(path.join(outA, 'developer', 'index.html'), 'utf8');

test('the build writes both pages, the loader, samples, evidence and .nojekyll', async () => {
  const files = await allFiles(outA);
  for (const expected of ['index.html', 'developer/index.html', '.nojekyll', 'playground/loader.mjs', 'playground/samples/hello.gu', 'evidence/index.html']) {
    assert.ok(files.includes(expected), expected);
  }
  assert.deepEqual(resultA.files.slice().sort(), files);
});

test('building twice gives identical bytes', async () => {
  const outB = await tmp('site-b-');
  await buildSite({ ...real, outDir: outB });
  const [a, b] = [await allFiles(outA), await allFiles(outB)];
  assert.deepEqual(a, b);
  for (const file of a) assert.deepEqual(await readFile(path.join(outA, file)), await readFile(path.join(outB, file)), file);
});

test('an unresolved token stops the build and names it', async () => {
  const srcDir = await tmp('src-');
  await writeFile(path.join(srcDir, 'shell.html'), '<!doctype html><title>t</title><body>{{slot:nope}}</body>');
  await assert.rejects(buildSite({ ...real, srcDir, outDir: await tmp('out-') }), /unresolved token: \{\{slot:nope\}\}/);
});

test('each page has exactly one script and it is the playground module', () => {
  assert.equal([...html.matchAll(/<script/g)].length, 1);
  assert.ok(html.includes('<script type="module" src="playground/loader.mjs"></script>'));
  assert.equal([...devHtml.matchAll(/<script/g)].length, 1);
  assert.ok(devHtml.includes('<script type="module" src="../playground/loader.mjs"></script>'));
});

test('the view switch is a native radio pair, Plain checked on the main page and Developer on /developer/', () => {
  assert.ok(html.includes('<input type="radio" name="view" id="view-plain" class="sr-only" checked>'));
  assert.ok(html.includes('<input type="radio" name="view" id="view-dev" class="sr-only">'));
  assert.ok(devHtml.includes('<input type="radio" name="view" id="view-plain" class="sr-only">'));
  assert.ok(devHtml.includes('<input type="radio" name="view" id="view-dev" class="sr-only" checked>'));
  assert.ok(html.includes('<fieldset class="viewswitch"><legend class="sr-only">Reading view</legend>'));
  assert.ok(html.includes('<label for="view-plain">Plain English</label>'));
  assert.ok(html.includes('<label for="view-dev">Developer</label>'));
  assert.ok(devHtml.includes('<meta name="robots" content="noindex">'));
  assert.ok(!html.includes('noindex'));
});

test('the CSS holds the view rules, reduced motion and print', () => {
  assert.match(html, /body:has\(#view-dev:checked\)\s*\.v-plain\s*\{[^}]*display:\s*none/);
  assert.match(html, /body:not\(:has\(#view-dev:checked\)\)\s*\.v-dev\s*\{[^}]*display:\s*none/);
  assert.match(html, /@media\s*\(prefers-reduced-motion:\s*reduce\)/);
  assert.match(html, /@media\s*print/);
  assert.match(html, /@layer\s+reset,\s*base,\s*layouts,\s*components,\s*states,\s*responsive,\s*print;/);
});

test('nothing in the page asks the network for anything', () => {
  for (const page of [html, devHtml]) {
    const scrubbed = page.replaceAll('xmlns="http://www.w3.org/2000/svg"', '');
    assert.doesNotMatch(scrubbed, /(?:href|src)="(?:https?:)?\/\//);
    assert.doesNotMatch(scrubbed, /url\(\s*['"]?https?:/);
    assert.doesNotMatch(scrubbed, /@import/);
  }
  assert.equal(resultA.report.remoteRequests, 0);
});

test('the page declares every section anchor exactly once', () => {
  for (const id of ['top', 'try', 'why', 'lifecycle', 'read', 'permissions', 'receipts', 'python', 'agents', 'story', 'footer']) {
    assert.equal([...html.matchAll(new RegExp(`id="${id}"`, 'g'))].length, 1, id);
  }
});

test('the report counts real bytes', () => {
  const { report } = resultA;
  assert.ok(report.htmlBytes > 1000 && report.cssBytes > 1000);
  assert.equal(report.jsBytes, Buffer.byteLength(readFileSync(path.join(SITE_ROOT, 'src', 'playground', 'loader.mjs'), 'utf8')));
  assert.ok(report.wasmBytes > 1_000_000);
});
