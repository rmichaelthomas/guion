import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { SITE_ROOT } from '../tools/lib/paths.mjs';
import { loadModules, compile, run, describe, LIMITS } from '../src/playground/loader.mjs';

const PLAYGROUND = path.join(SITE_ROOT, 'public', 'playground');
const SAMPLES = path.join(SITE_ROOT, 'src', 'playground', 'samples');
const modules = await loadModules(
  await readFile(path.join(PLAYGROUND, 'guion_compiler.wasm')),
  await readFile(path.join(PLAYGROUND, 'guion_vm.wasm')),
);
const samples = JSON.parse(await readFile(path.join(SAMPLES, 'samples.json'), 'utf8'));

async function execute(source, limits) {
  const compiled = compile(modules, source);
  if (!compiled.ok) return { stage: 'compile', ...compiled };
  return { stage: 'run', ...run(modules, compiled.artifact, limits) };
}

for (const sample of samples) {
  test(`sample "${sample.id}" behaves as recorded`, async () => {
    const result = await execute(await readFile(path.join(SAMPLES, `${sample.id}.gu`), 'utf8'));
    assert.equal(result.ok, sample.expect.ok, JSON.stringify(result));
    if (sample.expect.stdout !== undefined) assert.equal(result.stdout, sample.expect.stdout);
    if (sample.expect.code) assert.equal(result.code, sample.expect.code);
    if (sample.expect.line) assert.equal(result.line, sample.expect.line);
  });
}

test('limits are the documented ones', () => {
  assert.deepEqual(LIMITS, { sourceBytes: 65536, steps: 10000, outputBytes: 65536 });
});

test('a tighter step budget stops a program that would otherwise finish', async () => {
  const result = await execute(await readFile(path.join(SAMPLES, 'hello.gu'), 'utf8'), { steps: 3, outputBytes: 65536 });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'R_STEP_LIMIT');
});

test('empty source is refused with a code', () => {
  const result = compile(modules, '   \n');
  assert.deepEqual([result.ok, result.code], [false, 'E_SOURCE_EMPTY']);
});

test('source over 64 KiB is refused and says the limit', () => {
  const result = compile(modules, 'x'.repeat(65537));
  assert.equal(result.ok, false);
  assert.equal(result.code, 'E_SOURCE_TOO_LARGE');
  assert.match(result.message, /65536/);
});

test('a project import is refused because the browser has no files', () => {
  const result = compile(modules, 'import "tools.gu"\nfn main() -> Int:\n    return 0\n');
  assert.deepEqual([result.ok, result.code], [false, 'E_IMPORT_PROJECT_REQUIRED']);
});

test('the module survives a failure and runs the next program', async () => {
  await execute('fn main() -> Int:\n    return 1 + "a"\n');
  const result = await execute(await readFile(path.join(SAMPLES, 'hello.gu'), 'utf8'));
  assert.equal(result.stdout, 'Hello, Guion!\n');
});

test('describe gives every outcome a headline and a detail in plain words', async () => {
  const outcomes = [];
  for (const sample of samples) outcomes.push(await execute(await readFile(path.join(SAMPLES, `${sample.id}.gu`), 'utf8')));
  outcomes.push(compile(modules, ''), compile(modules, 'x'.repeat(65537)), { ok: false, stage: 'load', code: 'E_LOAD', message: 'fetch failed' });
  for (const outcome of outcomes) {
    const text = describe(outcome);
    assert.ok(text.headline.length > 0 && text.detail.length > 0, JSON.stringify(outcome));
  }
  const denied = describe(outcomes.find((o) => o.code === 'R_HOST_UNSUPPORTED'));
  assert.match(denied.detail, /grants nothing/);
  const loop = describe(outcomes.find((o) => o.code === 'R_STEP_LIMIT'));
  assert.match(loop.detail, /10,000/);
});

test('run() calls a named entry with typed Text arguments and returns its value', async () => {
  const compiled = compile(modules, 'fn main() -> Int:\n    return 0\n\nfn greet(name: Text, punctuation: Text) -> Text:\n    return "hello " + name + punctuation\n');
  assert.equal(compiled.ok, true, JSON.stringify(compiled));
  const result = run(modules, compiled.artifact, {}, { entry: 'greet', arguments: ['world', { type: 'Text', value: '!' }] });
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(result.value, 'hello world!');
});

test('run() with no options still runs main with no arguments', async () => {
  const compiled = compile(modules, 'fn main() -> Int:\n    return 7\n');
  const result = run(modules, compiled.artifact);
  assert.equal(result.ok, true);
  assert.equal(result.value, '7');
});

test('run() reports an entry the program does not have', async () => {
  const compiled = compile(modules, 'fn main() -> Int:\n    return 7\n');
  const result = run(modules, compiled.artifact, {}, { entry: 'missing', arguments: [] });
  assert.equal(result.ok, false);
  assert.equal(result.code, 'R_EXPORT_UNKNOWN');
});

const BUSY = 'fn main() -> Int:\n    return 0\n\nfn busy() -> Int:\n    var n = 0\n    while n < 5000:\n        set n = n + 1\n    return n\n';

test('steps above 10,000 are refused unless allowRaisedSteps is set, and then they are used', async () => {
  const compiled = compile(modules, BUSY);
  assert.equal(compiled.ok, true, JSON.stringify(compiled));
  const refused = run(modules, compiled.artifact, { steps: 200000 }, { entry: 'busy', arguments: [] });
  assert.equal(refused.ok, false);
  assert.equal(refused.code, 'E_ABI_LIMIT');
  const allowed = run(modules, compiled.artifact, { steps: 200000, allowRaisedSteps: true }, { entry: 'busy', arguments: [] });
  assert.equal(allowed.ok, true, JSON.stringify(allowed));
  assert.equal(allowed.value, '5000');
  assert.ok(allowed.steps > 10000);
});
