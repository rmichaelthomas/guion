import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { SITE_ROOT } from '../tools/lib/paths.mjs';

const PLAYGROUND = path.join(SITE_ROOT, 'public', 'playground');
const HELLO = 'fn main() -> Int:\n    let greeting = "Hello, Guion!"\n    print(greeting)\n    return 0\n';

async function instantiate(name) {
  const bytes = await readFile(path.join(PLAYGROUND, name));
  return (await WebAssembly.instantiate(bytes, {})).instance.exports;
}

function put(exports, allocate, data) {
  const pointer = allocate(data.byteLength);
  assert.notEqual(pointer, 0, 'allocation refused');
  new Uint8Array(exports.memory.buffer, pointer, data.byteLength).set(data);
  return pointer;
}

test('the VM module reports ABI 1.x', async () => {
  const vm = await instantiate('guion_vm.wasm');
  assert.equal(vm.guion_wasm_abi_version() >>> 16, 1);
});

test('the compiler adapter and the VM run a hello program end to end', async () => {
  const compiler = await instantiate('guion_compiler.wasm');
  const vm = await instantiate('guion_vm.wasm');

  const source = new TextEncoder().encode(HELLO);
  const sourcePointer = put(compiler, compiler.allocate_source, source);
  const status = compiler.compile_source(sourcePointer, source.byteLength);
  compiler.release_source(sourcePointer, source.byteLength);
  assert.equal(status, 0);
  const artifact = new Uint8Array(compiler.memory.buffer, compiler.result_artifact_ptr(), compiler.result_artifact_len()).slice();
  assert.ok(artifact.byteLength > 0);

  const request = new TextEncoder().encode(JSON.stringify({
    abi: { major: 1, minor: 0 }, entry: 'main', arguments: [], capabilities: [],
    limits: { steps: 10000, outputBytes: 65536 },
  }));
  const artifactPointer = put(vm, vm.allocate_input, artifact);
  const requestPointer = put(vm, vm.allocate_input, request);
  vm.execute_artifact_v1(artifactPointer, artifact.byteLength, requestPointer, request.byteLength);
  const response = JSON.parse(new TextDecoder().decode(new Uint8Array(vm.memory.buffer, vm.result_response_ptr(), vm.result_response_len())));
  vm.release_input(artifactPointer, artifact.byteLength);
  vm.release_input(requestPointer, request.byteLength);
  assert.equal(response.ok, true, JSON.stringify(response));
  assert.equal(response.stdout, 'Hello, Guion!\n');
});

test('wasm.build.json hashes match the files', async () => {
  const record = JSON.parse(await readFile(path.join(PLAYGROUND, 'wasm.build.json'), 'utf8'));
  assert.match(record.rustc, /^rustc 1\./);
  for (const name of ['guion_compiler.wasm', 'guion_vm.wasm']) {
    const bytes = await readFile(path.join(PLAYGROUND, name));
    assert.equal(record.files[name].sha256, createHash('sha256').update(bytes).digest('hex'), name);
    assert.equal(record.files[name].bytes, bytes.byteLength, name);
  }
});
