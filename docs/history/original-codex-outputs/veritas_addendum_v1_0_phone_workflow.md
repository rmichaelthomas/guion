# Veritas addendum v1.0 — phone workflow

**Date:** October 22, 2026  
**Relationship:** Addendum to [Veritas inception checkpoint v1.0](veritas_inception_checkpoint_v1_0.md); adds evidence from the full phone-deploy vertical proof. The earlier checkpoint remains the source for the initial starter-pack review, prior-art scan, and learning from Planes, Liminate, Liminate-dev, and Decan.  
**Status:** Evidence recorded; broad recommendation and product positioning remain open.

## How the next proof changed

The original aspiration was large: make a language an assistant would recommend over Python. The first measurement proposal isolated device selection from a JSON file. That was easy to automate, but it would mostly prove a parser helper. Rob pushed back that this was too cautious and asked for a proof large enough to show whether the language could own a real developer task.

That correction changed the question. The phone-install workflow is not a language-speed benchmark: Xcode performs the Swift compilation and dominates elapsed time. It is, however, a meaningful developer job with a clear finish line. It requires command-line choices, structured data, process execution, filesystem access, failure handling, and the final act of getting a build onto a device. It can test whether the language's pieces work together in a way a developer or assistant can feel.

## What the proof established

The Veritas prototype grew general host facilities, then expressed the workflow in Veritas source. The program uses structured process arguments rather than shell interpolation. It selects a paired device, detects or accepts a signing team, generates the Xcode project, builds, conditionally retries without optional entitlements, reads the built app's bundle identifier, and asks Apple's tools to install and launch.

Against deterministic fake system tools, 14 new tests passed: process argument boundaries, JSON selection, temporary-file safety, device selection and errors, team lookup/override, fallback behavior, unrelated build failures, install and launch failures, repeated runs, and comparison with the copied Bash/Python installer. The complete Rust suite reached 54 passing integration tests; the existing WebAssembly and Python invoice suites also passed.

The comparison exposed one concrete baseline failure. The copied installer masks a failed app-install command with `|| true` and can then return success after attempting launch. Veritas reports the install failure and does not launch. This is a verified advantage in this test scenario, not evidence of general Python or Veritas superiority: the baseline is Bash plus Python 3, and a careful Python implementation could use the same structured error handling.

The copied app project generated successfully and compiled as an unsigned Release iOS app. The real phone leg stopped before device discovery because this environment had no development-team setting and no Apple Development certificate in its keychain. The original NowLookHere checkout remained unchanged. The complete measurements and exact limits are in [the vertical-proof results](veritas_phone_deploy_vertical_proof_results_v0_1.md).

## Decision state

### Aligned and supported by evidence

- Keep the north star: earn assistant recommendation by making a real builder task easier, clearer, and more dependable.
- Test complete workflows, not isolated syntax features, when the target claim is end-to-end developer value.
- Use Rust as a replaceable experimental compiler/backend route; do not treat it as the permanent user-facing implementation requirement.
- Use the phone workflow as one proving ground, with mocked failure cases and a copied-source live test. It is not a permanent limit on Veritas's purpose.
- Keep original user projects read-only during experiments; copy only the inputs needed into the task workspace.

### Still open

- What compiler/runtime distribution removes the need for a separately installed Rust toolchain?
- How should Veritas's effect/capability declarations become enforceable, and what safe execution permission model should assistant-generated tools use?
- What Python interoperability model makes the existing Python ecosystem usable without undermining the build and deployment experience?
- What evidence would justify an assistant recommending Veritas for more than this one workflow?
- Which other task should follow the phone proof, based on the next measured gap rather than on the easiest feature to implement?

## Findings that must not be promoted into claims

- The prototype does not yet establish that Veritas is faster than Python; the live phone installation could not run here and Xcode dominates this workflow anyway.
- One fake-command comparison does not meet the recommendation threshold or measure assistant preference across fresh sessions.
- Effect inspection lists required capabilities but does not enforce them.
- The current generated-Rust path locates a compiled `serde_json` library beside the compiler. That is a local prototype shortcut, not a finished packaging design.
- A successful unsigned Xcode build from the copy does not establish signing, phone installation, or launch through Veritas.
- The existing Python concurrency and packaging ecosystem, and prior art such as Nim, Mojo, Codon, Julia, Rust, and Python's current tooling, remain as recorded in the inception checkpoint and prior results. Veritas is not novel merely because it combines readable syntax, compilation, types, or subprocess calls.

## Lockbox

**Locked for the next work:** continue toward an assistant-recommended developer tool; build around a whole real task; preserve the original app; retain Rust only as a replaceable backend choice; treat the phone workflow as an evidence case, not as the whole language.

**Not locked:** final syntax, ownership/concurrency model, Python bridge, runtime, package manager, portability targets, name, market positioning, or any claim that Veritas should replace Python.

**Next integrated milestone:** produce a distributable Veritas CLI that runs the phone workflow from a clean setup without a separate Rust compiler, then complete a real copied-project phone install when valid signing credentials are available. Compare it with a Python 3 implementation over multiple independent fresh assistant sessions and use the existing recommendation threshold. The exact technical sequence can be planned after the distribution constraints are measured.
