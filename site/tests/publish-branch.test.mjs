import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { publish } from '../tools/publish-branch.mjs';

const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@example.com', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@example.com' } }).trim();

/** A source repository whose `origin` is a bare repository, with a site/public tree on main. */
async function fixture(files = { 'index.html': '<!doctype html><title>v1</title>', '.nojekyll': '' }) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'publish-'));
  const remote = path.join(dir, 'remote.git');
  const repo = path.join(dir, 'repo');
  git(dir, 'init', '--bare', '-b', 'main', remote);
  git(dir, 'init', '-b', 'main', repo);
  git(repo, 'remote', 'add', 'origin', remote);
  await mkdir(path.join(repo, 'site', 'public'), { recursive: true });
  await writeFile(path.join(repo, 'README.md'), 'private source\n');
  await mkdir(path.join(repo, 'crates'), { recursive: true });
  await writeFile(path.join(repo, 'crates', 'secret.rs'), 'fn main() {}\n');
  await commitSite(repo, files);
  git(repo, 'push', 'origin', 'main');
  return { dir, remote, repo };
}

async function commitSite(repo, files) {
  for (const [name, body] of Object.entries(files)) await writeFile(path.join(repo, 'site', 'public', name), body);
  git(repo, 'add', '-A');
  git(repo, 'commit', '-m', 'site change');
  git(repo, 'push', 'origin', 'main');
}

const branchTree = (remote) => git(remote, 'ls-tree', '-r', '--name-only', 'refs/heads/gh-pages').split('\n');

test('the first publish creates gh-pages holding only the contents of site/public', async () => {
  const { repo, remote } = await fixture();
  const result = await publish({ repo });
  assert.equal(result.status, 'published');
  assert.deepEqual(branchTree(remote).sort(), ['.nojekyll', 'index.html']);
  assert.equal(git(remote, 'rev-parse', 'refs/heads/gh-pages'), result.commit);
});

test('a later publish adds one commit on top, so the push never needs force', async () => {
  const { repo, remote } = await fixture();
  const first = await publish({ repo });
  await commitSite(repo, { 'index.html': '<!doctype html><title>v2</title>' });
  const second = await publish({ repo });
  assert.equal(second.status, 'published');
  assert.equal(git(remote, 'rev-parse', 'refs/heads/gh-pages^'), first.commit);
  assert.match(git(remote, 'show', 'refs/heads/gh-pages:index.html'), /v2/);
});

test('publishing an unchanged site is a no-op', async () => {
  const { repo, remote } = await fixture();
  const first = await publish({ repo });
  const again = await publish({ repo });
  assert.equal(again.status, 'unchanged');
  assert.equal(git(remote, 'rev-parse', 'refs/heads/gh-pages'), first.commit);
});

test('a dry run reports the commit it would make and pushes nothing', async () => {
  const { repo, remote } = await fixture();
  const result = await publish({ repo, dryRun: true });
  assert.equal(result.status, 'dry-run');
  assert.match(result.commit, /^[0-9a-f]{40}$/);
  assert.throws(() => git(remote, 'rev-parse', '--verify', 'refs/heads/gh-pages'));
});

test('it publishes what is on origin/main, not what is in the working tree', async () => {
  const { repo, remote } = await fixture();
  await writeFile(path.join(repo, 'site', 'public', 'index.html'), '<!doctype html><title>unsaved edit</title>');
  await publish({ repo });
  assert.match(git(remote, 'show', 'refs/heads/gh-pages:index.html'), /v1/);
});

test('it refuses a site/public that cannot be served from a branch root', async () => {
  const noPage = await fixture({ '.nojekyll': '' });
  await assert.rejects(publish({ repo: noPage.repo }), /index\.html/);
  const noMarker = await fixture({ 'index.html': '<!doctype html>' });
  await assert.rejects(publish({ repo: noMarker.repo }), /\.nojekyll/);
});

test('it refuses when the ref has no site/public at all', async () => {
  const { repo } = await fixture();
  git(repo, 'checkout', '-b', 'other');
  git(repo, 'rm', '-r', '-q', 'site');
  git(repo, 'commit', '-m', 'no site');
  git(repo, 'push', 'origin', 'other');
  await assert.rejects(publish({ repo, ref: 'origin/other' }), /site\/public/);
});
