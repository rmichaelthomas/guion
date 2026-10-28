import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { renderContentSources, renderInstrumentMap, familiesIn } from '../tools/agent-docs.mjs';
import { REPO_ROOT, SITE_ROOT, allhereRepo } from '../tools/lib/paths.mjs';

const agentsDir = path.join(SITE_ROOT, 'agents');
const read = (file) => readFileSync(path.join(agentsDir, file), 'utf8');
const index = JSON.parse(read('index.json'));
const html = readFileSync(path.join(SITE_ROOT, 'public', 'index.html'), 'utf8');
const claims = JSON.parse(readFileSync(path.join(SITE_ROOT, 'src', 'claims.json'), 'utf8'));
let allhere = null;
try { allhere = allhereRepo(); } catch { /* AllHereHTML checkout not available */ }

test('the index names real documents and the guides name every command in it', () => {
  assert.equal(index.kind, 'guion_site_agent_index');
  assert.equal(index.schema_version, 1);
  for (const [name, relative] of Object.entries(index.documents)) assert.ok(existsSync(path.join(SITE_ROOT, relative)), `${name}: ${relative}`);
  const guide = read('build-and-verify.md');
  for (const [name, command] of Object.entries(index.commands)) assert.ok(guide.includes(command), `build-and-verify.md is missing ${name}: ${command}`);
});

test('every relative link in the agent documents resolves', () => {
  for (const file of readdirSync(agentsDir).filter((name) => name.endsWith('.md'))) {
    for (const [, target] of read(file).matchAll(/\]\((?!https?:|#|mailto:)([^)\s]+)\)/g)) {
      const resolved = path.resolve(agentsDir, target.split('#')[0]);
      assert.ok(existsSync(resolved), `${file} links to ${target}`);
    }
  }
});

test('the agent documents are not part of the deployed site', () => {
  const deployed = readdirSync(path.join(SITE_ROOT, 'public'), { recursive: true }).map(String);
  assert.ok(!deployed.some((file) => file.includes('agents/')), 'site/public must not contain agent documents');
  assert.ok(!html.includes('site/agents'));
});

test('the instrument map uses every family and matches the built page', { skip: !allhere && 'AllHereHTML checkout not found' }, () => {
  const generated = renderInstrumentMap({ html, allhere });
  assert.equal(read('instrument-map.md'), generated, 'instrument-map.md is stale: run node site/tools/agent-docs.mjs');
  const families = familiesIn(html);
  assert.equal(families.size, 10, [...families].join(', '));
  for (const [, id] of html.matchAll(/data-ahi="(ahi\.[^"]+)"/g)) assert.ok(generated.includes(id), id);
});

test('the content-sources table lists every claim with its source files and matches claims.json', () => {
  const generated = renderContentSources({ claims });
  assert.equal(read('content-sources.md'), generated, 'content-sources.md is stale: run node site/tools/agent-docs.mjs');
  for (const claim of claims) {
    assert.ok(generated.includes(`\`${claim.id}\``), claim.id);
    for (const source of claim.sources) assert.ok(generated.includes(source.path), source.path);
  }
});

test('the guides carry the rules an agent must not break', () => {
  const all = readdirSync(agentsDir).filter((name) => name.endsWith('.md')).map(read).join('\n');
  for (const phrase of ['site/public', 'ALLHERE_REPO', 'checkClaims', 'rmichael@rmichaelthomas.com', 'No public Guion release has been published', 'AGENTS.md', 'never a bare number']) {
    assert.ok(all.includes(phrase), phrase);
  }
  assert.ok(existsSync(path.join(REPO_ROOT, 'site', 'agents', 'README.md')));
});
