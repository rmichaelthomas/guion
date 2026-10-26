# Veritas v0.1: independent test results

**Verdict:** Veritas is feasible as a language project, and the new prototype proves a small end-to-end slice: a typed source file is parsed and checked, compiled to native code through Rust, and compiled to WebAssembly. The uploaded Python files do not prove the claimed VM or native pipeline. The first script does not parse; the second runs a narrow AST demo, but its “VM” is an AST interpreter and its C output is hard-coded independently of the program. There is no evidence yet that Veritas is a better default than Python for builders.

The right next goal is a focused developer workflow that beats Python on a measured problem—clear static errors, reproducible builds, and easy distribution—while keeping Python libraries available through a deliberate bridge. That is achievable. General Python replacement is a long-term ecosystem and compatibility challenge, not a milestone the current prototype can establish.

## Uploaded starter pack

### `veritas.py`

- It fails at parse time on line 113: `def peek(() -> Token:`. The `consume` and `parse` methods at lines 116 and 123 also have invalid/missing receiver parameters. The parser cannot run as submitted.
- In an isolated temporary copy, correcting those method signatures lets the toy interpreter print `40`; its C build then fails because generated C references undeclared `values_val` (generated C line 28). The script catches the compiler failure and still exits with status 0, so its exit code falsely suggests the pipeline worked.
- The source therefore demonstrates neither a functioning compiler nor a successful native/FFI path. No uploaded original was edited.

### `veritas_ultimate.py`

- Its one built-in demo prints `40`, but `VeritasVM` at lines 180–205 walks AST node classes and mutates a dictionary. It is an AST interpreter, not the claimed stack VM.
- `CExporter.export_source()` at lines 235–253 always emits the same loop and hard-coded `i > 5` filter. It does not lower the parsed AST. The script's `main()` writes C/header files; it does not compile them.
- I compiled the emitted C in a temporary directory and called its exported function: that fixed function returned `40`. I then changed the temporary program’s filter threshold from 5 to 7. The VM returned `27`, while the exported C still returned `40`. This isolates the core failure: the native output is disconnected from the source program.
- The lexer/parser recognize a tiny fixed statement vocabulary and silently discard unrecognized characters through token filtering. There is no general expression grammar, static type checker, or source-derived native code generation.

The chronology describes these demos as proof of a “stack-based VM” and C FFI, but the code does not support those claims. The accompanying claim that the community has “unanimously” identified a canonical set of Python flaws has no citations or reproducible evidence. Treat the chronology as brainstorming, not as a verified history or technical specification.

## What we built and verified

The new isolated prototype implements a small indentation-based, typed subset with records, functions, conditionals, loops, explicit mutation, `Result` error propagation, checked integer arithmetic, and an effect/capability inspection command. Its invoice example reads a pinned two-column format, applies exact tax-rate arithmetic, and can run natively or in a constrained WebAssembly adapter.

Fresh verification on this machine:

- `cargo fmt --check`: passed.
- `cargo test`: 40 integration tests passed (inspection 5, invoice 10, parser 6, native compilation 5, type checking 14).
- `node --test tests/wasm_invoice.mjs`: 5/5 passed, covering native/WASM parity, malformed input, invalid UTF-8, input capacity, and rejection of host-file-I/O code for WASM.
- `python3 -m unittest evaluation/python/test_invoice.py`: 7/7 passed for the Python baseline.
- The H7 invoice fixture contains 5,000 rows. An independent exact calculation agrees with the program: subtotal 264,348,770 cents; due at 8% is 2,854,966.72. The other checked totals are 10%: 2,907,836.47; 0%: 2,643,487.70; 25%: 3,304,359.63; and 8.25%: 2,861,575.44.

This is a real, testable compiler prototype, but its native backend currently emits Rust and invokes `rustc`. Users need a compatible Rust toolchain to build it. WebAssembly needed an additional rustup toolchain/target and linker library setup on this Mac. The implementation does not yet include a custom VM, self-contained compiler/runtime, Python interoperability, general CSV support, general package manager, or broad concurrency facilities. It is a vertical slice, not an independently distributable language platform.

## Clean-start task comparison

Three independent sessions implemented the same invoice CLI in Veritas and three in Python. Each received the same base prompt, then the tax-rate follow-up after finishing the base task. The shared harness was run afresh against every saved result:

| Language | Sessions | Registered acceptance cases passed | Timings recorded |
|---|---:|---:|---|
| Veritas | 3 | 22/22 each | Two complete sessions: 129s and 131s; third base start time was invalid, so exclude it |
| Python | 3 | 22/22 each | 131s, 33s, and 97s |

That is 132/132 common case passes across the six final programs. The task shows the Veritas implementation can express this small program and its type/target machinery can support it. It does not show faster development: the first two complete Veritas sessions took about as long as Python session 1, and Python session 2 was faster. A useful speed comparison is not possible because the exact model/version identifier was not recorded, the third Veritas base timing is unreliable, and human review minutes were not captured. The comparison is exploratory and not a recommendation-threshold pass.

After the registered task, a large-number probe used a subtotal of `10^35` cents at 8%. Three Veritas results and one Python result reject the checked intermediate multiplication; two Python results reduce the rational factor first and produce `due 1080000000000000000000000000000000.00`. The final amount fits signed 128-bit cents even though the unreduced intermediate product does not. This is a useful edge-case design question: if the language promises exact arithmetic, should avoidable intermediate overflow be rejected or should the evaluator reduce safely before multiplying? Define that rule before widening the arithmetic contract.

## What is and is not already available

The broad ingredients are established. Rust already provides static typing, native compilation, concurrency, and a strong toolchain. WebAssembly is an established portable target. Mojo explicitly combines Python-oriented syntax/interoperability with high-performance systems and AI work; its current Python-to-Mojo integration documentation still calls that bridge early development. So Veritas is not novel merely because it combines readable syntax, compilation, Python access, a VM, or WASM. The differentiator would have to be a coherent workflow and measured user advantage.

The original diagnosis also overstates Python's current gaps:

- CPython has offered an optional GIL-disabled build since Python 3.13. Extension modules that are not ready can re-enable the GIL, so compatibility and ecosystem coverage remain real issues. This machine’s Python 3.14.7 build has the GIL enabled. [Python’s free-threading documentation](https://docs.python.org/3/howto/free-threading-python.html)
- Python already has `asyncio` for concurrent I/O and high-level network code, alongside thread, process, and (in recent versions) interpreter executors. The GIL constrains CPU-bound threads in the standard build, but does not eliminate concurrency options. [asyncio](https://docs.python.org/3/library/asyncio.html), [concurrent execution](https://docs.python.org/3/library/concurrent.futures.html)
- Python project tooling can already lock exact dependency resolutions across platforms with `uv.lock`. It does not remove every environment or native-binary concern, but “no reproducible install workflow exists” is not a fair premise. [uv project guide](https://docs.astral.sh/uv/guides/projects/)
- PyInstaller can bundle the interpreter and dependencies into one executable. Its artifact is specific to the operating system, architecture, and active Python version, so it is not a universal single binary. [PyInstaller operating modes](https://pyinstaller.org/en/latest/operating-mode.html)
- Mojo is a direct current comparison for the Pythonic-syntax/high-performance/interop vision. Its docs identify CPU/GPU and AI infrastructure as its focus and say Python interop is still in active development. [Mojo manual](https://docs.modular.com/mojo/manual/), [Mojo/Python interop status](https://docs.modular.com/mojo/manual/python/mojo-from-python/)

The common reason to keep choosing Python is not that every Python shortcoming is unsolved; it is that Python has broad installed libraries, mature tools, familiar behavior, and low switching cost. A new language needs a narrow situation where its complete workflow is measurably easier or safer, plus an escape hatch to Python. For an LLM assistant, a language recommendation must also be backed by reliable examples, libraries, documentation, formatter/type-checker behavior, and a path to debug generated failures. Novelty alone will not make an assistant recommend it.

## Is Rust justified?

Rust is justified for the compiler, parser, static checker, code generator, runtime components, and native/WASM toolchain where memory safety and performance matter. It does not follow that every Veritas user should need Rust installed. If a future goal is “one Veritas executable installs, checks, builds, tests, and packages projects,” keep the current Rust backend behind a stable compiler interface and plan to ship a bundled compiler/toolchain after the language surface stabilizes. Building a custom VM now would add a second execution path before the language has enough programs to validate its semantics.

## Next decisions and work

1. Specify arithmetic semantics, including intermediate overflow, signed rounding, and the range contract for `Int` and `Money`; add regression cases for both accepted and rejected boundaries.
2. Expand the corpus beyond one invoice task: JSON input, files, a small HTTP service, a data transformation, and a package with a dependency. Compare complete working outcomes, diagnostics, setup, and review time—not just syntax.
3. Make comparisons reproducible: log exact model/runtime versions, prompts, timestamps, setup friction, all human interventions, and review time. Pre-register acceptance criteria and run multiple repetitions.
4. Prioritize the eventual “recommended by an assistant” goal through the developer experience: compact, trustworthy compiler diagnostics; deterministic project builds; formatter/test/lint/package in one command; verified Python library interop; stable docs and examples. Build each only when a representative task proves its value.
5. Revisit novelty and positioning after the workflow comparison. Today the evidence supports a promising research prototype and a feasible direction, not a Python replacement or a recommendation over Python.

## Evidence locations

The prototype source and tests are in the sibling `work/veritas-prototype` repository. The clean-start prompts, shared harness, cases, implementation snapshots, and per-session notes are under its `evaluation/` folder. The original uploaded files remain unchanged in their temporary ChatGPT preview locations.
