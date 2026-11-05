import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { SITE_ROOT } from '../tools/lib/paths.mjs';

const decode = (html) =>
  html
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x([0-9a-f]+);/gi, (_, code) => String.fromCodePoint(parseInt(code, 16)))
    .replace(/&#(\d+);/g, (_, code) => String.fromCodePoint(Number(code)))
    .replace(/&quot;/g, '"')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ');

// The page is built from the vendored fragments, not from instruments.json. After editing
// instruments.json, run `node site/tools/vendor-instruments.mjs`; this test fails until you do.
test('every vendored fragment carries the current text from instruments.json', () => {
  const { slots } = JSON.parse(readFileSync(path.join(SITE_ROOT, 'src/instruments.json'), 'utf8'));
  let checked = 0;
  for (const slot of slots) {
    const body = slot.content?.body;
    const file = path.join(SITE_ROOT, 'vendor/allhere/fragments', `${slot.id}.html`);
    if (slot.mode !== 'render' || typeof body !== 'string' || !existsSync(file)) continue;
    const fragment = decode(readFileSync(file, 'utf8'));
    assert.ok(fragment.includes(body.replace(/\s+/g, ' ')), `${slot.id}: fragment is stale; run node site/tools/vendor-instruments.mjs`);
    checked += 1;
  }
  assert.ok(checked >= 2, 'expected to check the release and ecosystem boxes at least');
});
