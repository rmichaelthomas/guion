import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { SITE_ROOT } from '../tools/lib/paths.mjs';

const source = await readFile(path.join(SITE_ROOT, 'src', 'playground', 'loader.mjs'), 'utf8');

test('the loader handles [data-pg-load] buttons, using only text APIs', () => {
  assert.ok(source.includes('[data-pg-load]'), 'no handler for [data-pg-load]');
  assert.ok(source.includes('dataset.pgLoad'), 'does not read the example name');
  assert.doesNotMatch(source, /innerHTML|outerHTML|insertAdjacentHTML|document\.write|eval\(/, 'only textContent-style writes are allowed');
});
