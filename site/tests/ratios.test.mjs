import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { renderRatioChart } from '../tools/ratios.mjs';
import { normalize } from '../tools/claims.mjs';
import { REPO_ROOT, SITE_ROOT } from '../tools/lib/paths.mjs';

const ratios = JSON.parse(readFileSync(path.join(SITE_ROOT, 'src', 'content', 'ratios.json'), 'utf8'));

test('every plotted ratio and interval end appears in the report it cites', () => {
  for (const entry of ratios) {
    const text = normalize(readFileSync(path.join(REPO_ROOT, entry.source), 'utf8'));
    for (const key of ['ratio', 'low', 'high']) assert.ok(text.includes(entry[key].toFixed(3)), `${entry.id}.${key} = ${entry[key].toFixed(3)} not in ${entry.source}`);
    assert.ok(entry.low <= entry.ratio && entry.ratio <= entry.high, entry.id);
  }
});

test('the chart plots faster and slower results with equal weight', () => {
  const svg = renderRatioChart(ratios);
  assert.match(svg, /^<svg[^>]*role="img"/);
  assert.ok(svg.includes('<title'));
  for (const entry of ratios) assert.ok(svg.includes(entry.label), entry.label);
  assert.ok(svg.includes('same time as Python'));
  assert.ok(svg.includes('Guion faster') && svg.includes('Guion slower'));
  assert.ok(ratios.some((entry) => entry.ratio > 1) && ratios.some((entry) => entry.ratio < 1));
});

test('positions follow a log scale: a ratio of 1 is at the reference line and 0.5 is as far left as 2 is right', () => {
  const svg = renderRatioChart([
    { id: 'a', label: 'half', ratio: 0.5, low: 0.5, high: 0.5 },
    { id: 'b', label: 'one', ratio: 1, low: 1, high: 1 },
    { id: 'c', label: 'two', ratio: 2, low: 2, high: 2 },
  ]);
  const cx = (id) => Number(new RegExp(`data-id="${id}"[^>]*cx="([\\d.]+)"`).exec(svg)[1]);
  const line = Number(/data-reference="1"[^>]*x1="([\d.]+)"/.exec(svg)[1]);
  assert.ok(Math.abs(cx('b') - line) < 0.01);
  assert.ok(Math.abs((line - cx('a')) - (cx('c') - line)) < 0.01);
});

test('the chart text is escaped', () => {
  const svg = renderRatioChart([{ id: 'x', label: 'A <b>&</b> B', ratio: 1, low: 1, high: 1 }]);
  assert.ok(!svg.includes('<b>'));
});

const inspectExample = JSON.parse(readFileSync(path.join(SITE_ROOT, 'src', 'content', 'inspect-example.json'), 'utf8'));
const binary = path.join(REPO_ROOT, 'target', 'release', 'guion');

test('the shown inspect report has the fields the page explains', () => {
  for (const key of ['required_effects', 'manifest_grants', 'grant_assessment', 'capability_claim']) assert.ok(key in inspectExample.report, key);
  assert.equal(inspectExample.report.grant_assessment.network_http, 'present_but_target_dynamic');
  assert.equal(inspectExample.report.capability_claim, 'static_bytecode_reachability_not_sandbox');
});

test('the shown inspect report is what the built binary prints for that program', { skip: !existsSync(binary) && 'no release binary' }, () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'inspect-'));
  writeFileSync(path.join(dir, 'main.gu'), inspectExample.program);
  writeFileSync(path.join(dir, 'guion.toml'), inspectExample.manifest);
  const run = spawnSync(binary, ['inspect', '--json', 'main.gu'], { cwd: dir, encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  assert.deepEqual(JSON.parse(run.stdout), inspectExample.report);
});
