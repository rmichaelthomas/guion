# Guion browser runtime ABI

This crate exposes the same portable production VM used by native Guion. ABI 1.0 adds typed browser calls while retaining the earlier pointer/i64 exports. It grants no browser or host capabilities by default.

- [ABI 1.0 specification](SPEC.md)
- [Execution plan and verification status](EXECUTION_PLAN.md)
- [Strict request schema](schema/v1-request.schema.json)
- [Response schema](schema/v1-response.schema.json)
- [Text request/response fixtures](fixtures/)
- [Browser-call example program](examples/typed-browser.gu)
- [Node Wasm ABI smoke script](../../evaluation/completion/test_browser_wasm_abi_v1.mjs)

## Run the ABI smoke

From the repository root, with the `wasm32-unknown-unknown` Rust target installed:

```sh
cargo test -p guion-vm-wasm --locked --offline
cargo build -p guion-vm-wasm --target wasm32-unknown-unknown --release --locked --offline
cargo run --locked --offline -p guion-cli -- build docs/browser-wasm-abi/examples/typed-browser.gu --output /tmp/typed-browser.gub
node evaluation/completion/test_browser_wasm_abi_v1.mjs /tmp/typed-browser.gub target/wasm32-unknown-unknown/release/guion_vm_wasm.wasm
```

The smoke invokes `greet(Text) -> Text` and `main() -> Int` from the actual built module, checks captured output and capability denial, and verifies that invalid caller pointers receive a structured failure rather than being dereferenced.

## Minimal browser embedder

The host owns artifact loading, and selects which compiled entry and typed values to pass. The module requires no imports.

```js
const { instance } = await WebAssembly.instantiateStreaming(
  fetch("/assets/guion_vm_wasm.wasm"),
  {},
);
const vm = instance.exports;
if ((vm.guion_wasm_abi_version() >>> 16) !== 1) {
  throw new Error("Unsupported Guion Wasm ABI");
}

function run(artifactBytes, request) {
  const requestBytes = new TextEncoder().encode(JSON.stringify(request));
  const artifactPtr = vm.allocate_input(artifactBytes.length);
  const requestPtr = vm.allocate_input(requestBytes.length);
  if (!artifactPtr || !requestPtr) {
    if (artifactPtr) vm.release_input(artifactPtr, artifactBytes.length);
    if (requestPtr) vm.release_input(requestPtr, requestBytes.length);
    throw new Error("Guion Wasm input limit reached");
  }
  try {
    new Uint8Array(vm.memory.buffer, artifactPtr, artifactBytes.length).set(artifactBytes);
    new Uint8Array(vm.memory.buffer, requestPtr, requestBytes.length).set(requestBytes);
    const status = vm.execute_artifact_v1(
      artifactPtr, artifactBytes.length, requestPtr, requestBytes.length,
    );
    const responseBytes = new Uint8Array(
      vm.memory.buffer, vm.result_response_ptr(), vm.result_response_len(),
    ).slice(); // Copy before the next Wasm call.
    const response = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(responseBytes));
    if (status !== (response.ok ? 0 : 1)) throw new Error("Invalid Guion Wasm status");
    return response;
  } finally {
    vm.release_input(artifactPtr, artifactBytes.length);
    vm.release_input(requestPtr, requestBytes.length);
  }
}

const response = run(artifactBytes, {
  abi: { major: 1, minor: 0 },
  entry: "greet",
  arguments: [{ type: "Text", value: "Ada" }],
  capabilities: [],
  limits: { steps: 1000, outputBytes: 1024 },
});
if (!response.ok) throw new Error(`${response.code}: ${response.message}`);
```
