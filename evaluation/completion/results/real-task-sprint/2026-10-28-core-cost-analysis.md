# Guion JSONL and HTTP cost analysis

Date: 2026-10-28
Reviewed source: branch codex/guion-core-sprint-spec at 41d3b48 (implementation at 7c7b8a9).
Method: source inspection, existing tests, frozen inputs, raw baseline/final/profile reports. No new performance experiment or implementation was performed for this analysis. This document distinguishes observed mechanisms from measured attribution.

## Conclusion

There is a concrete architectural path forward, but the retained evidence does not establish one definitive patch that closes both gaps. The JSONL application exercises interpreted, boxed general-purpose data operations and substantial accounting/dispatch work. The HTTP measurement combines API, program, compiler-input, and binary differences. Another parser micro-optimization or an assumed zero-copy conversion is not justified as the complete answer.

The next sprint must isolate costs first, then change the general runtime paths that own them, and prove closure on the unchanged useful application. A faster rewritten benchmark alone does not establish a faster core.

## What the evidence establishes

| Finding | Basis | Implication |
| --- | --- | --- |
| JSONL is primarily execution cost rather than compilation | jsonl-runtime-profile.json reports 540.2ms execution vs 4.6ms compile/cache at 20k | Source startup optimization cannot close this gap |
| No native functions execute in the JSONL profile | The same profile reports zero native compilations/functions | CPU benchmark wins do not establish fast general data processing |
| Native eligibility propagates failure through calls | native_backend.rs eligibility loop rejects callers of ineligible functions | One unsupported helper can keep a larger useful function interpreted |
| VM execution consumes an atomic instruction budget and handles boxed values | vm-core execute_vm_frame, Budget::consume, Op::Load | Dispatch, wrapper clones, and accounting are candidates for attribution, not proven CPU percentages |
| 200k rows produce about 31.5 million successful allocation-budget charge events and 3.918GB estimated gross charges | jsonl-runtime-profile.json | These are logical accounting events, not actual allocator calls or peak resident memory |
| Read-only median is 0.3601s; JSONL parse/line iteration is 1.2202s | jsonl-decomposition.json, same input and paired cohort | Parsing is costly relative to reading, but cannot be called the dominant cost of the entire roughly five-second application |
| Two local JSONL candidates did not give reliable gains | optimization-01/02 reports and patches | Reserved-key scanning and date-key concatenation are not established solutions |
| Latest JSONL artifact/Python paired ratios are 4.632 at 20k and 7.113 at 200k | linux-arm64-final.json | Both sizes must be acceptance workloads; the existing aggregate gate only requires the 20k lane |
| Typed HTTP conversion copies the body into Text | http_response_text_value validates borrowed bytes then constructs Arc<str>; legacy into_text_json consumes Vec through String::from_utf8 | A copy exists, but its share of the observed 6.4% end-to-end difference is unmeasured |
| HTTP comparison uses base binary + legacy source versus candidate binary + typed source | benchmark_core_workloads.py measure_http | 1.064x is an end-to-end workflow comparison, not an isolated decoder regression |
| Typed fixture imports std:http and std:types; legacy fixture has no imports; timeout inputs also differ | http_request_legacy.gu and http_response_json.gu | Uncached compilation and policy inputs must be controlled before attributing the difference |

The read/parse/full-program measurements come from different programs and captures. Their differences are useful for prioritization, not an additive CPU-time decomposition. Even eliminating the entire measured parse-only phase would leave a substantial full-program cost; exact savings require a same-cohort experiment.

## JSONL data flow and candidate fixes

`evaluation/completion/applications/jsonl-summary/main.vr` streams through std:jsonl, validates service and timestamp fields, formats each accepted timestamp, converts its text to a character list to obtain a date, repeatedly reads and updates JSON aggregates in nested maps, then materializes the output. The retained profile-input copy shows these same operations. `data_reference.py` uses Python's C JSON/date routines, tuple dictionary keys and mutable aggregate lists. Equal fixture outputs do not mean equal operation counts or equal general input semantics: Python parse_event accepts only its named UTC form, while Guion timestamp handling is broader.

The general runtime has existing sharing and map assignment optimizations; do not assume all collections are deep-copied. JSON get shares Arc children, and map_set recognizes immediate assignment back to its local. However, Op::Load still clones boxed Result/Option wrappers and charges logical copy quotas; the interpreter clones each instruction; Budget uses atomic updates; JSON set and finish_shared_json can traverse/recompute metrics for values. The parser builds Arc nodes and a BTreeMap, checks duplicate keys, scans reserved keys and validates metrics. Each is observable in code; none has a measured share of the total execution time yet.

Prioritize: (1) attribute VM dispatch/accounting and host operations; (2) optimize the dominant general path with preserved quotas and cancellation; (3) if remaining execution is dispatch dominated, extend native execution across the specific mixed-value/host-call boundary rather than tuning unrelated scalar benchmarks. Use phase ablations for validation/time/group updates to select the owning path. Parser metrics fusion is justified only if parser/metrics time is large enough; it must preserve duplicate-key, exact-number, reserved-key, depth, collection and text-limit behavior.

Do not claim that Arc::try_unwrap alone gives zero-copy HTTP text: Vec<u8> and Arc<str> use different ownership representations. Do not remove logical copy charges or instruction checks to win a benchmark. Do not implement a special-purpose JSONL summarizer builtin or route this workload through Python.

## HTTP data flow and candidate fixes

The byte request returns HttpResponse(status, headers, Arc<Vec<u8>>). http_response_text_value checks the text limit, validates UTF-8, charges allocation and copies into Text. The std:http helper executes Result inspection/unwrapping and invokes json_parse, which scans/parses and validates JSON metrics. The legacy response consumes its body Vec into a String before constructing a JSON envelope.

First compare legacy and typed programs on the same final executable, with equivalent timeout/body/host settings, in source and prebuilt-artifact modes. Add repeated in-process synthetic-response lanes to remove sockets, startup and compilation. Measure payload sizes including empty, 1KiB, 64KiB and 1MiB and preserve valid/invalid UTF-8 cases. Separate decode, JSON parse, helper/wrapper and network time.

If import/compile time owns the delta, improve reachable-standard-library compilation or compiler caching without changing runtime APIs. If wrapper dispatch owns it, lower or inline the small helper safely. If body conversion owns it, evaluate direct checked Bytes-to-JSON parsing behind the existing std:http signature; preserve UTF-8 error mapping, max_text_bytes, cumulative allocation policy and parser error positions/messages. An internal new operation requires normal catalog/compiler/format/bootstrap work; it is not assumed free. A new Text storage representation is a larger alternative, not the first patch.

## Tests and evidence limits

Existing runtime HTTP tests cover valid/non-2xx, invalid UTF-8, text ceilings, malformed JSON, denied hosts, timeouts and request-size/response-size policies. Parser tests cover shared/generic equivalence, reserved keys, duplicate keys and malformed JSON. These establish behavior, not timing attribution. Add allocation-budget boundary/aliasing tests for any conversion ownership change, and VM/native parity, cancellation and exact budget boundary tests for any execution/accounting change.

The original stored short-lane binary did not reproduce the frozen source/toolchain rebuild. Use the documented rebuilt baseline or rebuild both sides identically and retain hashes. The final report's Python comparisons are not a complete before/after comparison with the previous Guion build. Historical macOS and current Linux Python results must remain separate.

## North star and closure

Guion should make useful, Python-free agent applications fast while preserving understandable APIs and enforced resource boundaries. Close the JSONL gap on both frozen sizes and remove the typed HTTP workflow penalty under controlled comparison. Keep the original useful workload frozen, add adversarial inputs and an unrelated JSON/map application to prevent specialization, and report both direct Guion before/after and Python comparisons with peak memory.

No external evaluator or transfer is a prerequisite for this work. Fresh-builder usability can be assessed separately within an authorized local setup; it does not gate performance implementation.

## Primary sources reviewed

- docs/superpowers/specs/2026-10-28-guion-core-north-star-sprint-design.md
- README.md; docs/status.md; language/stdlib/http.gu; language/stdlib/jsonl.gu
- crates/runtime/src/lib.rs: http_response_text_value, HTTP request host operations, JsonParse/JsonSet/JsonGet, SharedJsonSeed
- crates/vm-core/src/lib.rs: execute_vm_frame, Budget, allocation accounting, finish_shared_json
- crates/runtime/src/native_backend.rs: native eligibility propagation and infer_native_types
- crates/runtime/src/stdlib_data.rs: parse_timestamp
- crates/runtime/tests/http.rs; runtime unique_json_parser_tests
- evaluation/completion/performance/runner.py and data_reference.py
- evaluation/development/core_north_star_sprint/benchmark_core_workloads.py and profile-inputs/
- 2026-10-28-linux-arm64-final.json; 2026-10-28-short-http-final.json; jsonl-runtime-profile.json; jsonl-decomposition.json; optimization-01/02 reports and patches
