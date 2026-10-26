# Veritas phone-deploy vertical proof — results v0.1

**Date:** October 22, 2026  
**Status:** Completed as a technical feasibility proof; live phone installation is blocked by signing setup  
**Build scope:** [Vertical-proof description](veritas_phone_deploy_vertical_proof_v0_2.md)  
**Prior evidence:** [Inception checkpoint v1.0](veritas_inception_checkpoint_v1_0.md), [prototype results v0.1](veritas_prototype_test_results_v0_1.md), [workflow baseline v0.1](veritas_nowlookhere_workflow_baseline_v0_1.md)

## Verdict

**Feasible and fixable as a language project. This proof does not establish that Veritas should replace Python or that an assistant should recommend it yet.** The original Rust experiment was a starting point, not a binding architecture choice. For this proof, Rust worked as a practical compiler host and code-generation backend.

The larger test succeeded where the earlier device-picker proposal would have been too small: Veritas source expressed a real multi-step developer task, handled data and subprocesses, and was tested against both successful and failing paths. In the same fake-command setup, Veritas correctly stopped on an install error; the copied Bash/Python installer reported success after the same error.

The live phone step did not run. Veritas correctly stopped before device discovery because this execution environment has neither a `DEVELOPMENT_TEAM` value nor a matching Apple Development certificate in its keychain. A separate unsigned Xcode build of the copied app succeeded, which verifies the copy and its dependencies but does not verify signing, installation, or launch through Veritas.

## What changed

The prototype now supports the general facilities this workflow needed:

- typed JSON parsing and checked object/array/string access;
- command-line arguments, environment lookup, text and file operations, temporary files, path joining, and simple text helpers;
- shell-free process execution using a program name, argument list, and working directory, with captured output and exit status;
- effect inspection that reports environment, filesystem, and process capabilities;
- safe temporary-file creation that uses exclusive creation and sanitizes the suffix.

The source program implements `examples/install_phone.vr`. It selects a paired iPhone/iPad, accepts `--device NAME`, reads or detects the Apple team, generates the Xcode project, builds Release, retries only for the known optional-capability messages, reads the app bundle identifier, then installs and launches. It has no NowLookHere-specific compiler builtin.

The backend emits Rust and invokes `rustc`; this is not a custom Veritas VM or machine-code backend. The generated Rust links the compiler's `serde_json` library for JSON. The current implementation locates that compiled library beside the Veritas executable, which is adequate for this prototype's Cargo workspace but is not yet a reliable standalone distribution design.

## Evidence

| Claim | Result | Evidence and limit |
|---|---|---|
| Veritas can own a full workflow's decisions | **Pass in the mock environment** | Compiled and ran the source program against fake `xcrun`, `xcodebuild`, `xcodegen`, `security`, `openssl`, and `PlistBuddy` commands. |
| Device choice and argument boundaries work | **Pass** | Ignores an unpaired iPhone and a paired watch; defaults to the first paired iPhone; exact device-name selection works. A separate process test preserves spaces and an apostrophe without shell interpolation. |
| Failure paths are controlled | **Pass** | No device, missing requested device, unrelated build failure, failed retry, install failure, and launch failure all return a failure with step context. Install failure prevents launch. |
| Capability fallback is selective | **Pass** | The optional-capability error gets one retry without those entitlements. An unrelated build error does not retry; a failed retry does not install. |
| Temporary files are safe and cleaned | **Pass for tested paths** | Veritas uses unique exclusive temp-file creation, rejects suffix path traversal, and removes device JSON and fallback entitlements on successful runs. The copied baseline left its fallback entitlement file behind; the test removed that test artifact afterward. |
| Existing Bash/Python behavior is comparable | **Partial** | The copied installer completes the same mock success/fallback path. Its current `|| true` after the install-output pipeline masks an install failure, then it proceeds to launch and exits successfully. The fake comparison supplies the team ID directly to avoid a shell pipeline race in the test harness; Veritas's automatic certificate path is separately covered in the Veritas mock. |
| The selected app copy builds | **Pass unsigned** | `xcodegen generate` succeeded. `xcodebuild` resolved seven packages and built Release for generic iOS with `CODE_SIGNING_ALLOWED=NO`. This build was run directly in the copy, not by the Veritas program. DerivedData was removed after recording the result. |
| Veritas installs and launches the app on a real phone | **Not verified** | The Veritas command stopped at team discovery: no environment team ID and no matching Apple Development certificate were available. No real-device install or launch was attempted. |

The app source/build copy is under `work/nowlookhere-copy` and was about 2.7 MB after deleting the 2.2 GB temporary DerivedData build. It contains the app inputs, generated Xcode project, a placeholder for omitted Swift package test sources, and no Git history, signing keys, or generated DerivedData. The original `/Users/rmichaelthomas/strands` working-tree state was the same before and after: two pre-existing modified scripts and two pre-existing untracked docs; no files there were changed by this proof.

## Verification run

From `work/veritas-prototype`:

- `cargo fmt --all --check` — passed.
- `cargo test --offline` — 54 integration tests passed, including 14 new host-capability and phone-workflow tests.
- `node --test tests/wasm_invoice.mjs` — 5/5 passed.
- `python3 -m unittest evaluation/python/test_invoice.py` — 7/7 passed.

The live baseline timing remains the earlier observation: 19.16 seconds on a cached repeat, with Xcode build time dominating. This proof did not produce a new live timing or a fair language-speed comparison. It also did not run independent assistant sessions, capture human review time, or meet the pre-registered threshold for recommending Veritas over Python.

## What this answers

**Is it feasible?** A new language can be built and can own a useful real-world automation workflow. This is evidence for the engineering feasibility of a focused language/toolchain, not evidence for broad adoption.

**Is it fixable?** The measured issues are concrete. Missing signing setup is environmental. The prototype's compiler dependency lookup, non-enforced effects, narrow host APIs, Rust toolchain requirement, and limited error/resource-cleanup semantics are technical work that can be addressed or consciously bounded.

**Is it already done?** The combination of a compiler, typed readable syntax, JSON, process execution, and integrated checks has extensive prior art. The prior-art review in the inception checkpoint still stands. Veritas has no demonstrated novelty claim yet. A possible distinction is whether assistants can more reliably turn a request into a reviewed, runnable, deployable result using one coherent Veritas toolchain; that remains to be tested with independent builders.

**Is Rust justified?** Yes as a prototype implementation route: it gave us a working parser/typechecker/code generator and a native backend without designing a VM first. No as a settled user requirement. Users should not need Rust installed if Veritas is to become the one-command toolchain described in the north star.

## Limits and risks exposed by this proof

- Effect inspection is descriptive; it does not sandbox or enforce which files, environment variables, or commands a program may access. `process.exec` is powerful and needs a deliberate permission model before assistant-generated programs are called safe.
- The Veritas source is compact enough to follow, but the Xcode argument construction and fallback branch are still verbose. More language/library design is needed before claiming lower friction than Python.
- Every `veritas run` compiles generated Rust before execution. Rust compiler availability and startup cost remain part of the current developer experience.
- The JSON backend's adjacent-`serde_json` lookup is a prototype shortcut; packaging or runtime embedding must replace it before distribution.
- This task is macOS/iOS-specific. It says little about concurrency, cross-platform packaging, Python-library interoperability, or general software development.
- The comparison with the existing installer is a deterministic behavior comparison using fake tools. It is not a multi-session assistant recommendation experiment.

## Next milestone

Keep the north star and widen the proof. The next integrated milestone should turn this prototype into a distributable Veritas command that a clean environment can install and run without a separately installed Rust compiler, then complete the copied NowLookHere build/install/launch on a real device when signing credentials are available. Compare the whole assistant-produced task against Python 3 over multiple fresh sessions, recording correctness, failed attempts, review/correction effort, setup time, and deployment time. Do not claim a Python recommendation unless the prior threshold is met.

The immediate environment prerequisite for the real phone leg is an Apple Development certificate available to Xcode or a valid `DEVELOPMENT_TEAM` setting with signing credentials. The live run should continue to use only `work/nowlookhere-copy`.
