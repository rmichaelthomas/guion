import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { REPO_ROOT } from '../tools/lib/paths.mjs';

const workflow = readFileSync(path.join(REPO_ROOT, '.github', 'workflows', 'pages.yml'), 'utf8');

test('it deploys only site/public', () => {
  assert.match(workflow, /uses: actions\/upload-pages-artifact@v\d+\.\d+\.\d+\s+with:\s+path: site\/public\s*$/m);
  assert.ok(!/path:\s*(\.|\.\/|\/)\s*$/m.test(workflow), 'an artifact path of the whole repository would publish private source');
});

test('it runs on main when the site changes, and by hand', () => {
  assert.match(workflow, /branches: \[main\]/);
  assert.match(workflow, /- "site\/\*\*"/);
  assert.match(workflow, /workflow_dispatch:/);
  assert.ok(!/pull_request/.test(workflow), 'pull requests must not deploy');
});

test('it asks for the least permission Pages needs', () => {
  assert.match(workflow, /permissions:\s+contents: read\s+pages: write\s+id-token: write/);
  assert.ok(!/write-all|contents: write/.test(workflow));
});

test('every action is pinned to an exact released version', () => {
  const uses = [...workflow.matchAll(/uses: ([\w-]+\/[\w-]+)@(\S+)/g)];
  assert.ok(uses.length >= 5);
  for (const [, action, version] of uses) assert.match(version, /^v\d+\.\d+\.\d+$/, `${action}@${version}`);
});

test('it checks the site before it deploys, and queues deploys instead of cancelling one midway', () => {
  const tests = workflow.indexOf("node --test 'site/tests/*.test.mjs'");
  const check = workflow.indexOf('node site/tools/check-site.mjs site/public');
  const upload = workflow.indexOf('upload-pages-artifact');
  assert.ok(tests > 0 && check > tests && upload > check);
  assert.match(workflow, /concurrency:\s+group: pages\s+cancel-in-progress: false/);
});
