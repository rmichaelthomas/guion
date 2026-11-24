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

async function compileAndRun(source, limits) {
  const compiler = await instantiate('guion_compiler.wasm');
  const vm = await instantiate('guion_vm.wasm');
  const encoded = new TextEncoder().encode(source);
  const sourcePointer = put(compiler, compiler.allocate_source, encoded);
  const status = compiler.compile_source(sourcePointer, encoded.byteLength);
  compiler.release_source(sourcePointer, encoded.byteLength);
  assert.equal(status, 0, 'the program compiles');
  const artifact = new Uint8Array(compiler.memory.buffer, compiler.result_artifact_ptr(), compiler.result_artifact_len()).slice();
  const request = new TextEncoder().encode(JSON.stringify({
    abi: { major: 1, minor: 0 }, entry: 'main', arguments: [], capabilities: [], limits,
  }));
  const artifactPointer = put(vm, vm.allocate_input, artifact);
  const requestPointer = put(vm, vm.allocate_input, request);
  vm.execute_artifact_v1(artifactPointer, artifact.byteLength, requestPointer, request.byteLength);
  const response = JSON.parse(new TextDecoder().decode(new Uint8Array(vm.memory.buffer, vm.result_response_ptr(), vm.result_response_len())));
  vm.release_input(artifactPointer, artifact.byteLength);
  vm.release_input(requestPointer, request.byteLength);
  return response;
}

// The beacon cryptography, bit operations and JSON run in the built browser VM, with the same
// answers as the native runtime (crates/vm-wasm/tests/crypto.rs compares the two in full).
const CRYPTO_JSON = [
  'fn main() -> Int:',
  '    let digest = native_crypto_sha512(bytes_from_text("abc"))',
  '    print(native_hex_encode(digest))',
  '    match json_parse("[1000, [1, 2, 3]]"):',
  '        Ok(value):',
  '            let round = json_as_int(json_index(value, 0))',
  '            let size = json_len(json_index(value, 1))',
  '            print(f"{int_and(round, 255)} {int_shift_left(1, 40)} {size}")',
  '        Err(problem):',
  '            print("bad json")',
  '    return 0',
  '',
].join('\n');

test('the VM module runs crypto, bit operations and JSON', async () => {
  const response = await compileAndRun(CRYPTO_JSON, { steps: 10000, outputBytes: 65536 });
  assert.equal(response.ok, true, JSON.stringify(response));
  assert.equal(response.stdout, 'ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f\n232 1099511627776 3\n');
});

test('a step budget above the default needs the explicit option, up to a hard ceiling', async () => {
  const loop = 'fn main() -> Int:\n    var i = 0\n    while i < 3000:\n        set i = i + 1\n    return i\n';
  const stopped = await compileAndRun(loop, { steps: 10000 });
  assert.equal(stopped.code, 'R_STEP_LIMIT');
  const refused = await compileAndRun(loop, { steps: 100000 });
  assert.equal(refused.code, 'E_ABI_LIMIT');
  const raised = await compileAndRun(loop, { steps: 100000, allowRaisedSteps: true });
  assert.equal(raised.ok, true, JSON.stringify(raised));
  assert.equal(raised.value.value, '3000');
  const over = await compileAndRun(loop, { steps: 50000001, allowRaisedSteps: true });
  assert.equal(over.code, 'E_ABI_LIMIT');
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
