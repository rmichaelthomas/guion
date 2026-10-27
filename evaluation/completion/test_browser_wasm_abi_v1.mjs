// Exercise the versioned typed ABI against a built production VM module.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [artifactPath, vmPath] = process.argv.slice(2);
if (!artifactPath || !vmPath) {
  throw new Error("usage: node test_browser_wasm_abi_v1.mjs ARTIFACT.gub VM.wasm");
}

const artifact = new Uint8Array(await readFile(artifactPath));
const wasmBytes = await readFile(vmPath);
const requestFixture = JSON.parse(
  await readFile(new URL("../../docs/browser-wasm-abi/fixtures/request-text.json", import.meta.url), "utf8"),
);
const responseFixture = JSON.parse(
  await readFile(new URL("../../docs/browser-wasm-abi/fixtures/response-text.json", import.meta.url), "utf8"),
);
const wasmModule = new WebAssembly.Module(wasmBytes);
assert.deepEqual(WebAssembly.Module.imports(wasmModule), [], "browser VM must not import ambient capabilities");
const exportedNames = new Set(WebAssembly.Module.exports(wasmModule).map((entry) => entry.name));
for (const name of [
  "guion_wasm_abi_version",
  "allocate_input",
  "release_input",
  "execute_artifact",
  "execute_artifact_v1",
  "result_response_ptr",
  "result_response_len",
]) {
  assert.ok(exportedNames.has(name), `missing Wasm export ${name}`);
}
const vm = (await WebAssembly.instantiate(wasmModule, {})).exports;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
assert.equal(vm.guion_wasm_abi_version(), 0x0001_0000);
assert.equal(typeof vm.execute_artifact, "function", "legacy ABI must remain exported");

function invoke(request) {
  const requestBytes = encoder.encode(JSON.stringify(request));
  const artifactPointer = vm.allocate_input(artifact.length);
  const requestPointer = vm.allocate_input(requestBytes.length);
  assert.notEqual(artifactPointer, 0, "artifact allocation should be bounded and available");
  assert.notEqual(requestPointer, 0, "request allocation should be bounded and available");
  new Uint8Array(vm.memory.buffer, artifactPointer, artifact.length).set(artifact);
  new Uint8Array(vm.memory.buffer, requestPointer, requestBytes.length).set(requestBytes);
  const status = vm.execute_artifact_v1(
    artifactPointer,
    artifact.length,
    requestPointer,
    requestBytes.length,
  );
  const responsePointer = vm.result_response_ptr();
  const responseLength = vm.result_response_len();
  const response = JSON.parse(
    decoder.decode(new Uint8Array(vm.memory.buffer, responsePointer, responseLength)),
  );
  vm.release_input(artifactPointer, artifact.length);
  vm.release_input(requestPointer, requestBytes.length);
  assert.equal(status, response.ok ? 0 : 1);
  return response;
}

const legacyPointer = vm.allocate_input(artifact.length);
assert.notEqual(legacyPointer, 0);
new Uint8Array(vm.memory.buffer, legacyPointer, artifact.length).set(artifact);
assert.equal(vm.execute_artifact(legacyPointer, artifact.length, 1_000), 0);
assert.equal(
  decoder.decode(new Uint8Array(vm.memory.buffer, vm.result_output_ptr(), vm.result_output_len())),
  "Hello from typed Wasm!\n",
);
assert.equal(vm.result_return_value(), 7n);
vm.release_input(legacyPointer, artifact.length);

const greeting = invoke(requestFixture);
assert.deepEqual(greeting, responseFixture);

const main = invoke({
  abi: { major: 1, minor: 0 },
  entry: "main",
  arguments: [],
  capabilities: [],
});
assert.equal(main.ok, true, JSON.stringify(main));
assert.equal(main.stdout, "Hello from typed Wasm!\n");
assert.deepEqual(main.value, { type: "Int", value: "7" });

const denied = invoke({
  abi: { major: 1, minor: 0 },
  entry: "main",
  arguments: [],
  capabilities: ["document.read"],
});
assert.equal(denied.code, "E_CAPABILITY_UNSUPPORTED");

const invalidPointerStatus = vm.execute_artifact_v1(1, 1, 1, 1);
assert.equal(invalidPointerStatus, 1);
const invalidPointer = JSON.parse(
  decoder.decode(
    new Uint8Array(vm.memory.buffer, vm.result_response_ptr(), vm.result_response_len()),
  ),
);
assert.equal(invalidPointer.code, "E_ABI_POINTER");

console.log(JSON.stringify({ passed: true, typedArguments: true, typedResults: true, limitsAndCapabilities: true }));
