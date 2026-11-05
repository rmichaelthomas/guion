import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

const cli = JSON.parse(readFileSync(path.join(SITE_ROOT, 'src', 'content', 'cli.json'), 'utf8'));
const binary = path.join(REPO_ROOT, 'target', 'release', 'guion');

test('every command has a plain sentence, a developer sentence and a usage line', () => {
  assert.ok(cli.commands.length >= 16);
  for (const command of cli.commands) {
    for (const key of ['name', 'args', 'plain', 'dev']) assert.equal(typeof command[key], 'string', `${command.name}.${key}`);
    assert.ok(command.plain.length > 10 && command.dev.length > 10, command.name);
  }
});

test('the command list and version match the built guion binary', { skip: !existsSync(binary) && 'no release binary at target/release/guion' }, () => {
  // `guion --help` prints the usage line and a one-line description of each command, and exits zero.
  const run = spawnSync(binary, ['--help'], { encoding: 'utf8' });
  const usage = `${run.stdout}${run.stderr}`;
  const listed = /<([a-z|-]+)>/.exec(usage)[1].split('|');
  assert.deepEqual(cli.commands.map((command) => command.name), listed);
  assert.equal(execFileSync(binary, ['--version'], { encoding: 'utf8' }).trim(), cli.version);
});

test('every JSON report kind named exists in the agent index', () => {
  const index = JSON.parse(readFileSync(path.join(REPO_ROOT, 'docs', 'agents', 'index.json'), 'utf8'));
  const kinds = new Set(index.cli_json_reports.reports.map((report) => `${report.kind} v${report.schema_version}`));
  for (const command of cli.commands.filter((entry) => entry.report)) assert.ok(kinds.has(command.report), `${command.name}: ${command.report}`);
});
