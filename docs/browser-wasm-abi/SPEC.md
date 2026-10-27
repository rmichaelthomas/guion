# Guion browser runtime ABI 1.0

- **Owner:** `rmichaelthomas/guion`, `crates/vm-wasm`
- **Status:** ABI 1.0 is implemented on `codex/guion-browser-abi-contract`; release/merge is coordinated by the parent task.
- **Artifact format:** independent from the WebAssembly ABI. Current `.gub` format is version 42.

## Exports and compatibility

`guion_wasm_abi_version() -> u32` returns `(major << 16) | minor`; this implementation returns `0x0001_0000`. Hosts that encounter no version export have a pre-discovery module and may use the legacy interface only after their own compatibility checks.

The v1 interface adds these exports:

```text
execute_artifact_v1(artifact_ptr: u32, artifact_len: u32,
                    request_ptr: u32, request_len: u32) -> u32
result_response_ptr() -> u32
result_response_len() -> u32
```

Inputs are caller-owned buffers from `allocate_input(len)` and must be released with the exact pointer and length. The module tracks live allocations, caps outstanding input memory at 2 MiB and 16 buffers, and rejects pointers or lengths that do not match a live buffer. A call returns 0 for `ok: true` or 1 for a structured error; the JSON response is available in either case. The caller must copy the response before the next execution call.

All prior exports remain available: `allocate_input`, `release_input`, `execute_artifact`, output/error getters, step count, and the legacy `i64` return getter. Existing callers retain the integer-only invocation behavior. The compatibility ABI does not silently change `.gub` format semantics.

## Request and response

See the strict JSON Schemas in `schema/`. Requests reject unknown fields and duplicate struct keys. ABI 1.0 grants no capabilities; the request must specify an empty `capabilities` array. Limits may lower the module ceilings, never raise them.

```json
{
  "abi": {"major": 1, "minor": 0},
  "entry": "summarize",
  "arguments": [
    {"type": "Text", "value": "three notes"},
    {"type": "Int", "value": "3"}
  ],
  "capabilities": [],
  "limits": {"steps": 10000, "outputBytes": 65536}
}
```

Successful responses contain `abi`, `ok: true`, typed `value`, captured `stdout`, and consumed `steps`. Failure responses contain `abi`, `ok: false`, stable `code`, and a readable `message`; VM failures also include `stdout` and `steps`. Clients branch on `code` and do not parse message text.

### Wire types

Every value is tagged with `type` and, except `Unit` and `OptionNone`, a `value` payload.

| Guion value | Wire representation |
|---|---|
| `Int` | Canonical signed decimal string; no plus sign, leading zeros, or negative zero. Arbitrary precision up to 4,096 digits. |
| `Float64` | Exactly 16 hexadecimal IEEE-754 bits; finite values only. |
| `Bool`, `Text` | JSON boolean; UTF-8 string. |
| `Bytes` | Standard base64 string. |
| `Unit`, `Option[...].None` | `{ "type": "Unit" }`, `{ "type": "OptionNone" }`. |
| `List[T]` | Array of recursively tagged values. |
| `Map[T]` | Array of `{ "key", "value" }` entries sorted by unique UTF-8 key. |
| Record | `{ "type": "Record", "value": { "name", "fields" } }`; field entries follow the artifact's declared field order and names. |
| `Option.Some`, `Result.Ok`, `Result.Err` | One recursively tagged value. |
| `Json` | A JSON value, subject to the same depth, node, string, and collection caps. |

Task, host error, and live `NativeResource` runtime values are not transferable and return `E_ABI_VALUE_UNSUPPORTED`. The adapter does not coerce or truncate values.

## Resource ceilings

| Resource | Ceiling |
|---|---:|
| Artifact | 1 MiB |
| Request JSON | 256 KiB |
| Response JSON | 256 KiB |
| Execution steps | 10,000 |
| Captured output | 64 KiB |
| VM allocation | 16 MiB cumulative |
| Live caller input buffers | 16 buffers / 2 MiB total |
| Recursive wire depth | 64 |
| Wire value nodes | 4,096 |
| Collection items | 4,096 |
| Individual text/byte value | 64 KiB |
| Integer digits | 4,096 |

The effective VM ceilings are the minimum of caller request limits, module ceilings, and embedded artifact limits. The portable host denies filesystem, network, database, clock, process, and other unimplemented host operations. It does not inherit permissions from the native runtime.

Stable ABI error codes include `E_ABI_REQUEST`, `E_ABI_VERSION`, `E_ABI_ENTRY`, `E_ABI_VALUE`, `E_ABI_VALUE_UNSUPPORTED`, `E_ABI_LIMIT`, `E_ABI_POINTER`, `E_CAPABILITY_UNSUPPORTED`, and the shared VM's stable runtime codes such as `R_CALL_ARITY`, `R_CALL_TYPE`, `R_STEP_LIMIT`, and `R_HOST_UNSUPPORTED`.

## Host capabilities and source compilation

ABI 1.0 supports no host capabilities. An empty list is the only accepted grant; a non-empty list returns `E_CAPABILITY_UNSUPPORTED`. Event, document-state, and persistence bridges remain future proposals and require two independent browser workloads before a common contract is added. General-purpose DOM access, `eval`, implicit origin/network access, and ambient persistence are out of scope.

Browser-side `.gu` source compilation is not part of this ABI. The existing AllHere adapter compiles a single in-memory source file with explicit size/import restrictions and remains AllHere-owned. Guion will generalize source compilation only after another independent consumer needs the same project boundary.

## Verification and release requirements

The checked-in native tests cover typed execution, non-`Int` results, arbitrary-precision integers, all transferable value families, canonical encodings, structured errors, capability denial, depth and output limits, and unsupported native resources. `evaluation/completion/test_browser_wasm_abi_v1.mjs` exercises the built Wasm exports, input-buffer lifecycle, typed values, captured output, and invalid pointer rejection in Node's WebAssembly runtime.

Before publishing ABI 1.0, run Rust formatting, the complete workspace tests, strict Clippy, build `wasm32-unknown-unknown`, inspect required exports and the no-import set, and run both the legacy and v1 Wasm smoke scripts. Run a browser-engine smoke against the same compiled module when a browser runner is available. Release notes must keep the ABI major/minor and `.gub` artifact version separate.
