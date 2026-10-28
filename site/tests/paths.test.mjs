import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { SITE_ROOT, REPO_ROOT, allhereRepo } from '../tools/lib/paths.mjs';

test('SITE_ROOT is the site directory', () => {
  assert.ok(SITE_ROOT.endsWith(`${path.sep}site`));
});

test('REPO_ROOT is the Guion repository root', () => {
  assert.ok(existsSync(path.join(REPO_ROOT, 'Cargo.toml')));
  assert.equal(path.join(REPO_ROOT, 'site'), SITE_ROOT);
});

test('allhereRepo explains how to point at a missing checkout', () => {
  assert.throws(() => allhereRepo({ ALLHERE_REPO: '/nonexistent' }), /set ALLHERE_REPO/);
});

test('allhereRepo finds the sibling AllHereHTML checkout by default', { skip: !existsSync(path.join(path.dirname(REPO_ROOT), 'allherehtml', 'workshop', 'core.mjs')) && 'no sibling AllHereHTML checkout' }, () => {
  const found = allhereRepo({});
  assert.ok(existsSync(path.join(found, 'workshop', 'core.mjs')));
});
