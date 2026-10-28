import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SITE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const REPO_ROOT = path.dirname(SITE_ROOT);

/** The AllHereHTML checkout used to render instruments: $ALLHERE_REPO, else a sibling of this repository. */
export function allhereRepo(env = process.env) {
  const candidate = env.ALLHERE_REPO ? path.resolve(env.ALLHERE_REPO) : path.join(path.dirname(REPO_ROOT), 'allherehtml');
  if (!existsSync(path.join(candidate, 'workshop', 'core.mjs'))) {
    throw new Error(`AllHereHTML checkout not found at ${candidate}; set ALLHERE_REPO`);
  }
  return candidate;
}
