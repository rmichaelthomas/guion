import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { REPO_ROOT } from './lib/paths.mjs';

const REQUIRED_AT_ROOT = ['index.html', '.nojekyll'];

function git(repo, ...args) {
  try {
    return execFileSync('git', args, { cwd: repo, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  } catch (error) {
    throw new Error(`git ${args.join(' ')} failed: ${String(error.stderr ?? error.message).trim()}`);
  }
}

function tryGit(repo, ...args) {
  try {
    return git(repo, ...args);
  } catch {
    return null;
  }
}

/**
 * Publish `<ref>:site/public` as the root of the Pages branch.
 *
 * Works on git objects only, so it never touches the working tree or index and is safe to run from any checkout.
 * Each publish is one commit on top of the branch's current tip, so the push is never forced.
 */
export async function publish({ repo = REPO_ROOT, remote = 'origin', ref = `${remote}/main`, branch = 'gh-pages', dryRun = false } = {}) {
  git(repo, 'fetch', '--quiet', remote);

  const tree = tryGit(repo, 'rev-parse', '--verify', `${ref}:site/public`);
  if (!tree) throw new Error(`${ref} has no site/public directory to publish`);
  const entries = git(repo, 'ls-tree', '--name-only', tree).split('\n');
  for (const required of REQUIRED_AT_ROOT) {
    if (!entries.includes(required)) throw new Error(`site/public at ${ref} has no ${required}; a branch root cannot be served without it`);
  }

  const parent = tryGit(repo, 'rev-parse', '--verify', `refs/remotes/${remote}/${branch}`);
  const source = git(repo, 'rev-parse', '--short', ref);
  if (parent && git(repo, 'rev-parse', `${parent}^{tree}`) === tree) return { status: 'unchanged', commit: parent, tree, source };

  const message = `Publish site/public from ${source}`;
  const commit = git(repo, 'commit-tree', tree, ...(parent ? ['-p', parent] : []), '-m', message);
  if (dryRun) return { status: 'dry-run', commit, tree, source };

  git(repo, 'push', '--quiet', remote, `${commit}:refs/heads/${branch}`);
  return { status: 'published', commit, tree, source };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : undefined);
  const result = await publish({ remote: option('--remote'), ref: option('--ref'), branch: option('--branch'), dryRun: args.includes('--dry-run') });
  const verb = { published: 'Published', unchanged: 'Already published', 'dry-run': 'Would publish' }[result.status];
  console.log(`${verb} site/public from ${result.source}: ${result.commit}`);
}
