# SDD ledger — plan: outputs/veritas_foundation_implementation_plan_v0_2.md

Status: implementation started after user approval.

## Pre-flight interface scan
- Task A -> B: A produces grammar/semantics/diagnostic/artifact/host/interop contracts consumed by compiler/runtime. Ruling: close the first supported subset before code; stage advanced surfaces behind explicit unsupported diagnostics. Risk: later syntax additions require compatibility/version updates.
- Task B -> C/D/E: B produces compiler/runtime public APIs consumed by bridge, host/concurrency and CLI. Ruling: use one `compile` -> artifact -> `Runtime.load` -> `Instance.invoke/close` path; CLI and embedded API call it. Risk: contract churn if the first API is too narrow.
- Task A/B -> F: language examples, semantics and release interfaces drive ordinary workload/evaluation. Ruling: no app-specific language built-ins; examples exercise ordinary APIs. Risk: early workload coverage will be smaller than final gate.

## Work record
- Build root: `work/veritas-foundation/` (fresh isolated Git repository; source repos untouched).
- Environment: Rust 1.97.1, Cargo 1.97.1, Python 3.14.7, clang available; `uv` unavailable at start.
- Ruling: begin with dependency-free Rust crates and a versioned portable bytecode artifact; avoid network/package assumptions until tool discovery and lock contract are explicit. Risk: missing established parser/CLI crates may slow delivery, but no external crate acquisition is required for the foundational compiler.

### Package A
- [x] language and architecture contract, including explicit implemented/planned split
- [x] reusable prototype inventory

### Package B
- [ ] compiler, verifier/runtime and source debugging

### Package C
- [ ] Python bridge and dependency manager

### Package D
- [ ] host APIs and structured concurrency

### Package E
- [ ] CLI, docs and packaging

### Package F
- [ ] representative programs, evaluation kit and report

Ruling: the approved plan is organized as work packages A–F rather than numbered Task headings; `task-brief` cannot extract task ranges. Execute package-by-package from the approved plan and record package status here; no user approval gates between packages.

### Package A status
- [x] Initial executable grammar and semantics in `language/spec.md` (functions, types, locals, conditionals, loops, lists, operators, entry point, diagnostics, artifact behavior).
- [x] First example in `language/examples/sum.vr`.
- [x] Read-only reuse inventory in `docs/reuse-inventory.md`; invoice/phone codegen/preludes excluded.
- [x] Architecture and costs documented in `docs/architecture.md`.
- [x] Host API catalogue, file/HTTP capability boundaries, cancellation and effect semantics specified in `language/host-api.md`.
- [x] Python call schema, conversion matrix, framing, supervision and lock boundary specified in `language/python-abi.md`.
- [x] Numeric, Unicode, map, record, `Option`/`Result`, module and external declaration rules recorded in `language/spec.md`; unimplemented rules are clearly marked planned.

### Package B status
- [x] Parsing/type checking/lowering of current subset; source spans and stable error codes.
- [x] Versioned bytecode encoding, decoding, and stack/control-flow validation.
- [x] Runtime load/invoke/close; step/call/output limits; function calls; lists; source trace.
- [ ] Exact/full resource accounting, wildcard/nested matching, general inferred constructors, generated/metamorphic tests, and remaining work packages remain. The first shared cumulative estimated-allocation budget now covers host inputs, VM frames and operand-stack growth, core value creation, selected host/JSON/Python paths, output rendering, nested calls, and parallel workers. Module graphs, records, and exhaustive Option/Result statement matching are implemented in the supported subset.
- [x] `Ok`, `Err`, and postfix `?` work in Result-returning functions with exact error-type matching; file error source positions remain inspectable after propagation.
- [x] `Option[T]` constructors (`Some(value)` and `None`), presence testing, and checked `unwrap` work through source compilation, type verification, versioned artifact serialization, and VM execution. Artifact format is now version 9. Pattern matching and general constructor inference remain open.
- [x] `Map[Text, T]` supports homogeneous non-empty literals, typed function parameters and returns, lookup with a source-traced missing-key error, `map_has`, immutable `map_set`, sorted key iteration, `len`, collection/text resource limits, and version-10 artifact serialization with verifier coverage. Runtime tests cover duplicate-key rejection, copy-on-write behavior, deterministic iteration and relocation round-trip. Typed maps with bridge-supported item types convert recursively to/from Python dictionaries with string keys. Empty maps remain open.
- [x] Immutable JSON and list values use shared reference-counted storage, so VM local loads do not deep-copy whole collections. A generated JSON aggregation probe caught and verified a quadratic JSON-array-copy regression; see `evaluation/results/sales-report-storage-optimization.json`.

### Package C status
- [x] Versioned length-prefixed JSON sidecar protocol with request IDs, frame/nesting limits, exact integer transfer, output capture, exception details, timeouts, child termination and cleanup.
- [x] `extern python "module:callable" fn ...` declarations are typechecked, serialized in portable artifacts, and executed through the same runtime path as source and CLI programs.
- [x] Local development requires explicit interpreter selection through `HostPolicy` or `VERITAS_PYTHON`; no interpreter is discovered silently.
- [x] Managed project dependencies: exact CPython patch pin and package requirements in `veritas.toml`; pinned uv resolves and retains its native `uv.lock`; `veritas.lock` binds the manifest, uv version, Python version, target, and lock digest; explicit `sync --update` changes resolution; `run` verifies locks and syncs offline before starting the managed interpreter.
- [x] Locked Python runs reject a `veritas.lock` target that differs from the current host; the regression test failed against the prior implementation and passed after target validation was added.
- [x] The real locked uv integration test copies the checked-in, checksum-locked interop project and runs in offline mode. A fresh test-only resolver attempt exposed unavailable sandbox DNS; relying on PyPI during the full suite was removed. First-time dependency resolution remains verified separately by the actual managed interop project sync and package run.
- [x] Real host integration on macOS ARM64 used checksum-verified uv 0.13.4, managed CPython 3.14.8, pure-Python `requests==2.32.5`, and compiled `orjson==3.13.0`. The generated interop project decoded `List[Int]` through `orjson`, returned a requests user-agent string, and ran from the packaged CLI after `veritas sync`.
- [ ] Bundled toolchain assets beyond the current macOS ARM64 target, Bytes/Option/Result conversions, source keywords, native wheel relocation, and broader array/object-heavy compatibility remain. Typed maps and finite Float64 values now convert through the Python bridge.

### Package E status
- [x] `new`, `check`, `run`, `build`, `inspect`, `doc`, `doctor` basic commands through same compiler/runtime.
- [x] `.vrb` round-trip and execution with source removed.
- [x] Current-host bundle verifies and includes the pinned uv 0.13.4 executable, its MIT/Apache-2.0 license texts, CLI, docs, language examples, and a manifest-backed Python-interoperability project. Its checksum and extracted CLI examples were verified on macOS ARM64, including managed Python sync and offline locked run. CPython is acquired during project sync; self-contained/relocatable Python packages, signing/notarization and other targets remain open, so this is a local evaluation archive.
- [x] `new` creates a manifest-backed runnable project; project-directory commands read/validate the declared entry and reject path traversal.
- [x] `fmt` and `fmt --check` normalize line endings/trailing whitespace while preserving source comments and string contents; `test` isolates discovered tests in fresh instances.
- [x] `veritas run` and discovered tests read positive integer resource ceilings from `[runtime]` in `veritas.toml`; unsupported names and invalid values fail explicitly. The default allocation budget is 256 MiB per invocation.
- [ ] Syntax-aware layout formatting, custom test declarations, dependency manifest resolution, packaging/distribution lifecycle and executable documentation remain.

## Float64 milestone refresh (2026-10-22)

- [x] Finite `Float64` source literals, typed parameters/results, unary negation, arithmetic, comparisons, explicit `json_as_float`, exact version-11 artifact serialization/verification, VM execution, and Python bridge conversion are implemented. Integer/Float64 mixing and non-finite values are rejected; integer division remains unsupported.
- [x] `language/examples/float64.vr` demonstrates explicit JSON conversion, a typed function call, arithmetic, assertion, and output.
- [x] Regression coverage observed expected pre-implementation failures for decimal literal lexing and missing `json_as_float`, followed by passing end-to-end tests.
- [x] `VERITAS_TEST_UV_INTEGRATION=1 ... cargo test --workspace --offline` passed 59 tests, including the actual managed CPython integration with `requests` and `orjson`.
- [x] `cargo fmt --all -- --check`, Clippy with warnings denied, and `python3 -m unittest discover -s evaluation/tests -v` passed (2 tests).
- [x] Version-11 package smoke passed for source execution, artifact execution with source removed, bundled `doctor`, and managed Python sync/run from an extracted archive in a path with spaces and Unicode.

## Exact integer division milestone (2026-10-22)

- [x] Arbitrary-precision `Int` now supports `/` truncating toward zero, `//` floor division, and `%` remainder with the divisor's sign, satisfying `a == (a // b) * b + (a % b)`. Zero divisors raise `R_DIV_ZERO`.
- [x] Decimal long division is implemented directly over Veritas' arbitrary-precision integer representation, without narrowing through a machine integer. It precomputes digit multiples and uses binary search for quotient digits. The compiler, version-12 bytecode format, verifier, and VM support each operator.
- [x] Regression coverage observed the unsupported operator failure before implementation and now checks negative operand combinations, a 30-digit dividend, Float64 `/` remaining true division, artifact encode/decode/execute, and all three zero-divisor failures.
- [x] Full workspace passed 63 Rust tests, including 4,096-digit division and managed uv/CPython integration; formatting, Clippy with warnings denied, and the Python evaluation harness (2 tests) passed.
- [x] Version-12 package smoke passed for both numeric examples, version-12 artifact execution with source removed, `doctor`, and managed Python sync/run from the extracted archive.

### Package D status
- [x] `parallel_map` returns a typed `Result[List[U], TaskError]` with task index, code, message and trace available to guest code.
- [x] Worker failures stop new assignments, running worker VMs poll cancellation between instructions, the group joins workers, completed output is merged in input order, and worker instruction consumption is bounded while reserving budget for result handling.
- [x] JSON value type with exact integer preservation, stable object order, duplicate-key rejection, typed accessors, and depth/byte/item/integer limits.
- [x] JSON syntax and duplicate-key failures return `Result[Json, JsonError]` with stable code/message and line/byte-column accessors; size/depth/integer policy limits remain explicit runtime failures.
- [x] Filesystem read/write failures return `Result[..., FileError]` with inspectable codes/messages and source line/column; write calls are direct and may leave partial files on OS errors.
- [ ] HTTP, independent compute-vs-I/O evidence and broader cancellation/side-effect cases remain.

Development workload: `evaluation/development/sales_report/` is a manifest-backed project that reads project-local JSON, inspects typed file and parse Results, traverses a JSON array, sums exact integer fields, and reports count and total. The equivalent Python standard-library script produces the same expected output (`12`, `1524`). `python3 evaluation/development/run_known.py` records both outputs in machine-readable JSON. The current-host release binary ran the workload, and a versioned artifact copied with its fixture into `/private/tmp/veritas-sales-run/` returned the same output without source files. The release CLI also ran the sum, JSON extraction, parallel-map and file-copy examples; the copied file matched byte for byte. This is known correctness and local relocation evidence, not independent evidence.

Performance investigation: `evaluation/development/benchmark_sales_report.py` generates a larger temporary fixture and validates every output from Veritas source, Veritas artifact and Python runs. Before immutable JSON sharing, Veritas median runtime was about 100 ms at 1,000 rows and 2.46 s at 5,000; the initial 10,000-row attempt did not finish within roughly a minute. VM local reads copied the full JSON tree per loop iteration. JSON and list values now share immutable storage on clone. In the latest run, source-run medians (including source compilation each invocation) were about 6.4 ms at 1,000, 12.7 ms at 5,000, and 22.9 ms at 10,000; Python medians were about 32–36 ms. At 10,000 rows Veritas p95 (48 ms) exceeded Python's (42 ms), showing the tail is not better in every run. These are one-host, one-workload author measurements, not a recommendation result; details are in `evaluation/results/sales-report-storage-optimization.json` and the per-size raw reports.

Verification at the shared-storage milestone: `cargo test --workspace --offline` passed 43 tests; formatting, Clippy, the two Python benchmark-harness tests, known workload comparison, and `git diff --check` passed. Managed Python was then added and separately passed an actual integration test with real pinned uv and wheel installations. Final suite counts and outputs are being refreshed after that addition. JSON tests cover malformed parse and duplicate keys as typed errors, source locations, exact integers, canonical round-trip, typed access, and limits. File tests cover typed access-denial/path-escape reporting, source location after `?`, and successful read/write under a granted root. The failing-worker test checks `Err` task identity and trace, cancellation, and joining without assuming scheduler order. Python tests cover artifact execution, integer precision, output isolation, exception reporting, return validation, explicit configuration, and timeout cleanup. Remaining work-package validation is not complete.

## Prior language evidence reviewed read-only

- Planes' core-sufficiency report found that checking one entry file missed `when` in its imported lexer; its Horizon and renderer investigations use pre/post workloads under repeated interaction. Applied by requiring module-graph coverage and repeated/long-session tests before general claims.
- Liminate's actual pipeline and test discipline make syntax bounded, parsing observable, ambiguity explicit, and errors structured. Applied by requiring complete input consumption and stable diagnostics; its prose/domain-pack design is intentionally not imported.
- Decan's conformance matrix, loss-aware adapters, and three-consumer corpus distinguish exact slices from partial support. Applied by requiring capability/interop limits and multiple ordinary consumer programs before recommendation claims; its temporal core boundary is intentionally not imported.
- Details and scope boundaries are recorded in `docs/prior-language-build-lessons.md`. None of these source repositories was modified.

## Current verification refresh

- `cargo fmt --all -- --check` passed after managed dependency work.
- `VERITAS_TEST_UV_INTEGRATION=1 ... cargo test --workspace --offline` passed 50 tests across CLI, compiler, model, runtime, and Python bridge, including actual uv-managed CPython 3.14.8 with `requests` and compiled `orjson` wheels, Option construction/runtime/artifact round-trip, and target-lock rejection.
- `cargo clippy --workspace --all-targets --offline -- -D warnings` passed.
- `python3 -m unittest discover -s evaluation/tests -v` passed 2 tests.
- Host-target lock regression was observed failing before the fix and passing afterward. Option runtime integration was observed failing with `E_TYPE_UNKNOWN` before implementation and now passes, including the absent-value runtime error and context restriction.
- The known sales-report workload returned the same `12\n1524\n` output in Veritas and Python. Its report remains explicitly classified as a known development case, not independent evidence.
- `cargo clippy --workspace --all-targets --offline -- -D warnings` and `python3 -m unittest discover -s evaluation/tests -v` passed. A final fresh package smoke follows this refresh.
- Release archive built as `/private/tmp/Veritas RC 14 Ω/veritas-0.1.0-darwin-arm64.tar.gz`; SHA-256 `ec36cfea306972f3e00d0662097172cf7833da5c42da7317d6309eda77487539`, sidecar checksum verified. The extracted package contains the inception checkpoint, prior-language lessons, and Option example; bundled `doctor` and Option run passed. On the byte-identical executable from RC 12, sum, JSON, parallel, file-copy, managed Python sync and execution also passed; copied file matched byte-for-byte. A version-9 Option bytecode artifact ran from a different Unicode-named directory with no source file present.
- Foundation build checkpoint v1.1 continues the original v1.0 and approved v0.2 plan at `docs/veritas_checkpoint_v1_1_foundation_build.md`; it records the user goal, decisions, prior-language evidence, current competing tooling, refreshed build state, open gaps, and continuation order. It explicitly does not claim Veritas is generally recommendable yet.
- Map slice: the first integration test was observed failing with `E_SYNTAX` on `{`, then passed after parser, static typing, bytecode format v10, verifier, runtime operations, resource checks, CLI type display, docs, and example support were added. `cargo test --workspace --offline` passed 53 tests after this slice; final integration and package checks are being refreshed.
- Map-to-Python slice: the bridge test was observed failing with `R_PYTHON_CONVERSION`, then passed after the Rust frame encoder/decoder and Python sidecar gained recursive string-keyed dictionary handling. Direct bridge and source-level tests passed.
- Final refresh: `VERITAS_TEST_UV_INTEGRATION=1 VERITAS_UV=/private/tmp/veritas-uv-0.13.4/uv-aarch64-apple-darwin/uv UV_CACHE_DIR=/private/tmp/veritas-uv-eval/cache UV_PYTHON_INSTALL_DIR=/private/tmp/veritas-uv-eval/pythons cargo test --workspace --offline` passed 55 Rust tests, including the managed Python integration. `cargo fmt --all -- --check`, Clippy with warnings denied, and `python3 -m unittest discover -s evaluation/tests -v` passed (2 tests).
- Version-10 package RC17: `/private/tmp/Veritas RC 17 ν/veritas-0.1.0-darwin-arm64.tar.gz`; SHA-256 `32a06a31387498c7098d9d9e39616e89cef151e34786da93615c0243d07039ba`; sidecar matched. Extracted bundle `doctor`, Map source execution, bytecode build/run after deleting its source copy, and bundled managed Python sync/run passed. The latter printed `76` and `python-requests/2.32.5`. The package, checksum and checkpoint are copied under `outputs/veritas-foundation-rc17/`.


### Record milestone refresh (2026-10-22)

- [x] Added declared records with ordered typed fields, positional construction, field access, structural equality, artifact schema/value/instruction encoding (format v13), runtime type checks, and verifier validation of record schemas and operations.
- [x] Tests cover source compile → artifact encode/decode → VM execution; wrong constructor types, missing fields, unknown field types, duplicate fields, and invalid record indices.
- [x] `language/examples/records.vr` runs and prints `5\n3\n`; language contract and reference are updated.
- [x] `VERITAS_TEST_UV_INTEGRATION=1 ... cargo test --workspace --offline` passed all 66 Rust tests, including the pinned offline managed CPython/dependency integration. `cargo fmt --all -- --check`, Clippy with warnings denied, and the Python harness (2 tests) passed.
- [x] Format-13 RC21 package was rebuilt from release sources with checksum-pinned uv binary, extracted into a temporary directory, inspected, and its records source and source-free bytecode ran. Archive SHA-256: `b0f34c2eae5a0d650cb3c277cf4400892f2bd0a41935f270153fa9ef2b8bd835`.
- [x] Multi-file import graph resolves explicit relative imports under the project root, rejects cycles and path escapes, exposes only direct imports’ `pub fn`/`pub record` symbols, and diagnoses unimported/private names. Added nested module imports and records to CLI acceptance coverage.
- [x] Artifact format v14 stores source-file names and instruction source IDs. Compiler diagnostics and runtime traces identify the originating imported file, including after building bytecode and deleting source files. Runtime artifact verification rejects unknown source IDs and malformed source tables.
- [x] `language/examples/modules_main.vr` and `modules_math.vr` run through the CLI; the output is `44`.
- [ ] Namespaced/selective imports, matching, complete resource accounting, remaining work packages and release evaluation remain.


### Module graph and source-map milestone (2026-10-22)

- [x] Added project-level compiler API and connected CLI `check`, `run`, `build`, `fmt`, and `test` to the full entry module graph.
- [x] Tests cover public records/functions, private visibility, transitive import scope, imported compile/runtime error attribution, source-map round-trip in bytecode after source removal, cycles, and parent-path rejection.
- [x] The format-14 source table and `Span.source_id` are encoded/decoded and validated by the runtime verifier.

### Module graph milestone verification (2026-10-22)

- [x] `VERITAS_TEST_UV_INTEGRATION=1 VERITAS_UV=/private/tmp/veritas-uv-0.13.4/uv-aarch64-apple-darwin/uv UV_CACHE_DIR=/private/tmp/veritas-uv-eval/cache UV_PYTHON_INSTALL_DIR=/private/tmp/veritas-uv-eval/pythons cargo test --workspace --offline` passed all **67 Rust tests**, including managed CPython and dependency locking.
- [x] `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --offline -- -D warnings` passed. `python3 -m unittest discover -s evaluation/tests -v` passed **2 tests**.
- [x] The module acceptance graph verifies direct-public imports, unqualified/private/transitive visibility, public record construction, nested imports, cycle detection, lexical traversal rejection, compile/runtime error attribution, and source map persistence after source deletion.
- [x] RC22 current-host bundle was extracted to a temporary directory; `doctor`, records execution, module execution, format-14 build, and execution of the module artifact after hiding both source files passed. The archive SHA-256 sidecar verified: `15aa7059934b16230714dd2e5e87203f761ff1d2c3365f7976554a2377a32d30`.
- [x] RC22 package includes the v1.5 checkpoint and both module source examples. It remains an unsigned macOS ARM64 evaluation bundle, not a cross-platform or clean-machine deployment claim.

### Option and Result matching milestone (2026-10-22)

- [x] Added exhaustive statement matching for `Option[T]` (`Some(name)` / `None`) and `Result[T, E]` (`Ok(name)` / `Err(name)`). Either arm order is accepted; each bound value is scoped to its arm.
- [x] Lowering uses the existing typed variant-check and extraction operations. It evaluates the matched expression once and preserves normal control flow and mutable state across a non-returning match.
- [x] Regression tests cover both variant families, both arm orders, matching after artifact encode/decode, output, continuation after the match, missing arms, duplicates, and wrong-family variants.
- [x] The full offline workspace suite with pinned uv/CPython integration passed **69 Rust tests**. Formatting, Clippy with warnings denied, and both Python evaluation-harness tests passed.
- [x] `language/examples/matching.vr` runs through the CLI and prints `42`, `no value`, `17`, and `calculation failed`.
- [x] RC23 archive SHA-256 sidecar verified (`1ffff4c65d0f756b0cf140a8e4be3581e776ab0f0fc18c1cc1b832cad5b656c0`); extracted `doctor`, matching source run, format-14 build, and artifact run after moving the source file away all passed. The package contains the v1.6 checkpoint and matching example.
- [ ] Wildcards, nested patterns, matching other types, match expressions, general constructor inference, and complete resource accounting remain open.

### Runtime allocation-budget milestone (2026-10-22)

- [x] `HostPolicy` exposes a per-invocation `allocation_limit_bytes` budget (default 256 MiB) with stable `R_ALLOCATION_LIMIT` source-traced failure. Accounting is cumulative and estimated at known language-value creation points, frame locals, operand-stack growth and bounded host results; it is deliberately not described as exact RSS/native-allocation accounting.
- [x] Nested calls share one budget; parallel workers share a counter with caller headroom reserved. Top-level invocations reset the counter. Output rendering is bounded by remaining output allowance before appending to the instance buffer.
- [x] The CLI applies positive resource fields from a project's `[runtime]` manifest table to `run` and test execution, rejecting unknown names and invalid integers. A real CLI acceptance test configures a low allocation ceiling and observes `R_ALLOCATION_LIMIT`.
- [x] Test-first regressions cover aggregate temporary allocations across nested calls, reset on the next invocation, shared parallel allocation failure as `TaskError`, stack growth, print rendering, and manifest configuration.
- [x] The known sales-report workload still matches the Python standard-library reference (`12`, `1524`); `evaluation/results/development-sales-report.json` records both outputs and labels the case as non-independent evidence.
- [x] The full offline workspace with pinned uv/CPython integration passed **74 Rust tests**. `cargo fmt --all -- --check`, Clippy with warnings denied, and the Python evaluation harness passed (2 tests).
- [x] RC24 archive SHA-256 sidecar verified (`27990f2e83a85cf1d66e01a62bef71d61df222457ff8ec32944d40c79dc14e2d`); extracted `doctor`, matching, records and module examples passed; the matching example also built as format-14 bytecode and ran after its source was moved away. A temporary project with `allocation_limit_bytes = 1024` failed through the packaged CLI with `R_ALLOCATION_LIMIT`. The bundle contains v1.7 docs and matching source; README and checkpoint are copied alongside the archive in outputs.
- [x] `veritas-foundation-source-rc24.tar.gz` and its checksum sidecar (`559a4766e2b6a7192d13c9493b674168bafe223886ca3cc133f944bc8b17c45c`) deliver the editable workspace source, docs, examples and evaluation kit. After extracting that archive in a fresh temporary directory, the offline workspace suite passed all **74 Rust tests**, including managed CPython integration.
- [ ] Full resource accounting remains open for transient parser/native allocations, all compiler/host/bridge surfaces, effective-policy reporting, observed-memory stress and broader lifecycle integration.

### Portable artifact policy (2026-10-22)

- [x] Format 15 adds optional compiler version, build host, and numeric resource-limit metadata. `build` writes effective project/default limits; `inspect` reads `.vrb` and reports them.
- [x] Runtime applies each recorded limit as an upper ceiling by taking the stricter value with the host policy. Format 14 remains readable and has no embedded policy.
- [x] A CLI test demonstrated the gap first, then passed after the fix: build with a 1,024-byte allocation cap, copy only the artifact outside the project, inspect the recorded cap, and observe `R_ALLOCATION_LIMIT` at run time. The model test round-trips v15 metadata and v14 without metadata.
- [x] Full offline workspace suite passed **75 Rust tests**, including managed CPython integration; `cargo fmt --all -- --check`, warning-free Clippy, and Python evaluation harness (2 tests) passed.
- [x] Fresh current-host bundle passed `doctor`, matching, records and modules examples; format-15 policy metadata was inspected and enforced after copying only the artifact and deleting its source project.
- [x] Fresh source-archive extraction passed the same full offline workspace suite (75 Rust tests including managed Python). Archive checksums and package verification are recorded with RC25 outputs.

### Versioned source and lock provenance (2026-10-22)

- [x] Artifact format 16 adds build metadata schema 1 with compiler/build-host identity, SHA-256 for the exact compiled source graph, optional combined `veritas.lock` + `uv.lock` identity, and the effective runtime ceilings. Source bytes are hashed during compilation from the same buffers parsed by the compiler.
- [x] `inspect` reports schema, source digest, dependency lock digest or absence, and limits. Format 14 and 15 remain readable; unsupported artifact and metadata schema versions are rejected.
- [x] Test-first coverage confirms same-input build bytes match on the same host, edits to an imported module change the source digest, lock edits change only the lock digest, incomplete lock pairs fail, and artifact policy still follows a moved `.vrb`.
- [x] Full offline workspace passed **76 Rust tests** with managed Python; formatting, Clippy with warnings denied, and 2 Python evaluation tests passed. The known sales-report result was refreshed and still matches (`12\n1524\n`), labeled as non-independent development evidence.
- [x] Fresh RC26 package `doctor`, language examples, and moved format-16 artifact policy checks passed. Source archive extraction verification and both archive hashes will be captured in the RC26 milestone report.
### Package destination safety milestone — RC61 (2026-10-22)

- [x] Reproduced the dangling-symlink destination bug with a black-box JSON package regression; prior behavior emitted `E_PACKAGE_FAILED` because `Path::exists` followed the link.
- [x] Replaced follow-symlink preflight with `symlink_metadata`; occupied destinations, including dangling symlinks, now fail with `E_PACKAGE_OUTPUT_EXISTS` and preserve the link. macOS final publication uses atomic `renameatx_np(RENAME_EXCL)` so a destination created after preflight cannot be replaced.
- [x] Added an end-to-end Unicode-and-spaces package destination check: package, verify, and launch from a different working directory.
- [x] `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --offline -- -D warnings` passed; full workspace passed 119 Rust tests including managed Python integration. An initial restricted run failed only where socket/network access was required; the access-enabled run passed.
- [x] Added the v1.38 checkpoint and machine-readable result; RC61 current-host distribution and installed evaluation kit passed verification (22 Python acceptance checks).
- [ ] Release archive path validation, cross-platform atomic publication, and independent recommendation evaluation remain open.

### Release archive integrity milestone — RC62 (2026-10-22)

- [x] Inspection of the RC61 archive found 5,642 macOS AppleDouble entries and a Python bridge interpreter symlink pointing into the temporary package staging tree.
- [x] Added a tar validator for absolute/traversal paths, duplicates, special entries, link escapes/missing targets, and members nested below links; four adversarial tests pass.
- [x] Release packaging suppresses AppleDouble metadata and validates the actual archive before calculating its checksum.
- [x] Python bundle construction rewrites absolute `.venv` links to relative links resolving inside the package; regression test passes.
- [x] Formatting, strict Clippy and full workspace suite passed: 120 Rust tests, including managed Python integration. RC62 contains 5,637 validated archive entries, zero AppleDouble entries, and zero absolute symlinks; its checksum verifies and the installed 22-test acceptance kit passes. Fresh Unicode-path extraction, `doctor`, and two identical offline Python app launches pass.
- [ ] Dynamic runtime loader provenance, clean-machine validation, cross-platform packaging, and independent recommendation evidence remain open.

