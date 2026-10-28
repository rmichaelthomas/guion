import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, readdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { allhereRepo, SITE_ROOT } from '../tools/lib/paths.mjs';
import { renderSlot, vendor } from '../tools/vendor-instruments.mjs';

let allhere = null;
try { allhere = allhereRepo(); } catch { /* AllHereHTML checkout not available (for example in CI) */ }
const skip = !allhere && 'AllHereHTML checkout not found';
const slotsFile = JSON.parse(await readFile(path.join(SITE_ROOT, 'src', 'instruments.json'), 'utf8'));
const renderSlots = slotsFile.slots.filter((slot) => slot.mode === 'render');
const CHROME = [/Read how this piece works/, /How this .* works/, /instrument-contract/, /instrument-action/, /A line to begin with/, /Another part of the story/, /No executable artifact/, /The passage opens here/];

test('renderSlot renders a kinetic headline with the authored words', { skip }, async () => {
  const { html } = await renderSlot(allhere, {
    id: 'probe', mode: 'render', instrumentId: 'ahi.typography-reading.kinetic-headline',
    content: { heading: 'guion.', body: 'A language.', caption: 'Programming, re-examined' },
  });
  assert.match(html, /data-ahi="ahi\.typography-reading\.kinetic-headline"/);
  assert.match(html, /id="slot-probe"/);
  assert.match(html, /guion\./);
  assert.doesNotMatch(html, /<script/i);
  for (const pattern of CHROME) assert.doesNotMatch(html, pattern);
});

test('renderSlot rejects an unknown instrument and names it', { skip }, async () => {
  await assert.rejects(
    renderSlot(allhere, { id: 'x', mode: 'render', instrumentId: 'ahi.nope.nothing', content: {} }),
    /ahi\.nope\.nothing/,
  );
});

test('there is at least one render slot and every one is clean', { skip }, async () => {
  assert.ok(renderSlots.length >= 4);
  for (const slot of renderSlots) {
    const { html, css } = await renderSlot(allhere, slot);
    assert.doesNotMatch(html, /<script|https?:\/\/(?!www\.w3\.org\/2000\/svg)/i, slot.id);
    for (const pattern of CHROME) assert.doesNotMatch(html, pattern, slot.id);
    assert.doesNotMatch(css, /https?:|@import/, slot.id);
    // internal links must resolve inside the fragment
    for (const [, target] of html.matchAll(/href="#([^"]+)"/g)) assert.match(html, new RegExp(`id="${target}"`), `${slot.id}: dangling #${target}`);
  }
});

test('vendor is deterministic and records provenance', { skip }, async () => {
  const run = async () => {
    const outDir = await mkdtemp(path.join(os.tmpdir(), 'vendor-'));
    const result = await vendor({ allhere, slots: slotsFile.slots, outDir });
    return { outDir, result };
  };
  const first = await run();
  const second = await run();
  assert.deepEqual(first.result.files, second.result.files);
  assert.match(first.result.allhereCommit, /^[0-9a-f]{40}$/);
  const provenance = await readFile(path.join(first.outDir, 'PROVENANCE.md'), 'utf8');
  assert.ok(provenance.includes(first.result.allhereCommit));
  const catalogHash = createHash('sha256').update(await readFile(path.join(allhere, 'instruments/registry/catalog.json'))).digest('hex');
  assert.ok(provenance.includes(catalogHash));
  const fragments = await readdir(path.join(first.outDir, 'fragments'));
  assert.deepEqual(fragments.sort(), renderSlots.map((slot) => `${slot.id}.html`).sort());
  assert.ok(existsSync(path.join(first.outDir, 'PROVENANCE.md')));
});
