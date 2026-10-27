// Exercise the AllHereHTML browser compiler ABI against Guion's shared Wasm VM.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [compilerPath, vmPath] = process.argv.slice(2);
if (!compilerPath || !vmPath) throw new Error("usage: node test_browser_wasm.mjs COMPILER.wasm VM.wasm");
const compiler = (await WebAssembly.instantiate(await readFile(compilerPath), {})).instance.exports;
const vm = (await WebAssembly.instantiate(await readFile(vmPath), {})).instance.exports;
const decoder = new TextDecoder();

function text(module, ptr, len) {
  return decoder.decode(new Uint8Array(module.memory.buffer, module[ptr](), module[len]()));
}

function compile(source) {
  const bytes = new TextEncoder().encode(source);
  const pointer = compiler.allocate_source(bytes.length);
  assert.notEqual(pointer, 0);
  new Uint8Array(compiler.memory.buffer, pointer, bytes.length).set(bytes);
  const status = compiler.compile_source(pointer, bytes.length);
  compiler.release_source(pointer, bytes.length);
  if (status) return { error: text(compiler, "result_error_ptr", "result_error_len") };
  return { bytes: new Uint8Array(compiler.memory.buffer, compiler.result_artifact_ptr(), compiler.result_artifact_len()).slice() };
}

function run(bytes) {
  const pointer = vm.allocate_input(bytes.length);
  assert.notEqual(pointer, 0);
  new Uint8Array(vm.memory.buffer, pointer, bytes.length).set(bytes);
  const status = vm.execute_artifact(pointer, bytes.length, 32);
  vm.release_input(pointer, bytes.length);
  if (status) return { error: text(vm, "result_error_ptr", "result_error_len") };
  return { output: text(vm, "result_output_ptr", "result_output_len"), result: vm.result_return_value() };
}

const hello = compile('fn main() -> Int:\n    print("Hello from browser-side compilation!")\n    return 7\n');
assert.ok(hello.bytes, hello.error);
const execution = run(hello.bytes);
assert.equal(execution.output, "Hello from browser-side compilation!\n");
assert.equal(execution.result, 7n);
const imported = compile('import "other.gu"\nfn main() -> Int:\n    return 0\n');
assert.match(imported.error, /E_IMPORT_PROJECT_REQUIRED/);
const fileAccess = compile('fn main() -> Int:\n    let value = read_text("missing.txt")\n    return 0\n');
assert.ok(fileAccess.bytes, fileAccess.error);
assert.match(run(fileAccess.bytes).error, /R_HOST_UNSUPPORTED/);
console.log(JSON.stringify({ passed: true, compilerToVm: true, importRejected: true, hostOperationRejected: true }));
