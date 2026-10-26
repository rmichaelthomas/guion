# Veritas phone-deploy vertical proof v0.2

**Date:** October 22, 2026  
**Status:** Implemented as an experimental vertical proof; see `veritas_phone_deploy_vertical_proof_results_v0_1.md` for measured outcomes and limitations
**Relationship:** Extends Veritas Inception Checkpoint v1.0 and replaces the device-picker-only proposal in `veritas_device_picker_build_description_v0_1.md`. The device-picker proposal remains as a record of the narrower option; it is not the selected build scope.  
**Baseline:** [NowLookHere workflow timing](veritas_nowlookhere_workflow_baseline_v0_1.md)

## Intent

The north star is to earn a point where a capable assistant recommends Veritas over Python for a real builder job. The device-picker-only task would mostly prove JSON parsing. That is too small to meaningfully test the north star.

This larger proof asks a harder question: **Can an assistant use Veritas to turn one natural-language request into a reliable command that builds an existing iOS app, puts it on a phone, and opens it—without editing the app project or making the user correct the run command?**

This tests the whole job: language surface, compiler diagnostics, command-line experience, system-tool integration, safe handling of arguments and environment, failure recovery, and the user-facing result. It does not ask Veritas to compile Swift faster. Xcode will still compile the app; Veritas must show value in making the complete workflow clearer, less error-prone, or faster to complete.

## Starting evidence

The current NowLookHere installer completed three times on the connected phone:

- First observed run: 29.52 seconds; pre-run DerivedData state is unknown.
- Cached repeat: 19.16 seconds.
- Instrumented repeat: 21.88 seconds, comprising about 11.54 seconds for the initial build attempt, 4.63 seconds for the fallback build, 4.65 seconds to install, 0.50 seconds to launch, and under 0.3 seconds for device discovery and project generation.

The first Xcode build fails because Push Notifications and Associated Domains are not enabled for this app ID. The current installer then retries without those entitlements and succeeds. That fallback is an important real behavior the Veritas program must preserve or improve. The full baseline and its limitations are in the linked report.

## What the proof builds

Build a general Veritas program that performs the workflow currently handled by `scripts/install-device.sh`, using the same system tools but expressing its decision-making in Veritas source. It must:

1. Find an available paired iPhone/iPad, defaulting to the first eligible device or honoring an exact `--device NAME` selection.
2. Determine or accept the Apple development team using behavior equivalent to the current workflow.
3. Generate the Xcode project in an isolated copy of the app source.
4. Build the Release app for the selected device.
5. If the build fails specifically because the optional Push Notifications or Associated Domains capabilities are unavailable, retry without those two entitlements. Stop on unrelated build failures.
6. Install the resulting app on the selected phone and launch it.
7. Report each failure with the failed step and useful context; return a nonzero status on failure.

The program must use structured process arguments. It may not build a shell command by joining user-controlled text and passing it to a shell. Veritas source must contain the workflow logic: it cannot hide the installer in a shell script or call a device-install-specific builtin.

The source should be runnable as one documented command, such as `veritas run tools/install_phone.vr --device NAME`. The final exact CLI syntax may be refined during implementation without changing the acceptance behavior.

## Language and toolchain work included

The current prototype is not yet able to express this job. A successful proof will likely require a coherent first standard surface for:

- command-line arguments and environment variables;
- reading/writing files and temporary files;
- JSON values with checked object, array, string, and boolean access;
- launching a process with an argument list and working directory;
- capturing standard output/error and exit status;
- returning structured errors and cleaning temporary state;
- effect/capability inspection for file access, environment access, and process execution.

Use the existing Rust-hosted compiler as the experimental implementation route unless the design review finds a concrete blocker. This is a test choice, not a commitment that Rust is the permanent backend or a runtime users will install. Count the existing Rust toolchain setup in the comparison. The language facilities must stay general; no NowLookHere-specific device, Xcode, or entitlement logic belongs in Veritas builtins.

## Isolation and non-modification rules

- Never edit, generate project files in, or build inside `/Users/rmichaelthomas/strands`.
- Copy only the source and build inputs needed for this test into a dedicated workspace under this task. Exclude Git history, unrelated work, credentials, test secrets, caches, and generated artifacts. Use a synthetic identity/configuration where possible; never copy signing secrets.
- Generate Xcode project files and all build outputs only in that isolated copy, with a new DerivedData directory for controlled timing.
- Use fake `xcrun`, `xcodebuild`, `xcodegen`, `security`, and related commands for automated success/failure tests. Do not rely on a live device for those cases.
- The eventual live test runs only the completed Veritas-built tool against the isolated source copy. It may replace/refresh the installed app on the phone, as the current installer does, but must not change NowLookHere source files.

Before making the copy, inspect which build inputs are essential and confirm that excluded secret files are not needed. If a required signing/configuration value cannot be safely represented in the copy, stop the live-install part and report the limit; continue the fake-command tests.

## Acceptance and comparison

### Automated behavior

Run the Veritas workflow against fake system commands and synthetic device-list data. Cover at least:

- first eligible phone/tablet selection; watch and unpaired devices ignored; exact named-device selection;
- no paired device and missing requested device;
- no development team and a supplied team;
- normal build success;
- missing optional capabilities followed by successful fallback;
- unrelated build failure with no install or launch attempt;
- install failure with no launch attempt;
- launch failure with actionable output;
- paths/device names containing spaces and apostrophes, proving argument boundaries are preserved;
- repeat execution and cleanup of temporary files.

Run the same workflow cases against a Python 3 reference and the current shell/Python installer where applicable. The actual NowLookHere source stays outside the test's writable area.

### End-to-end behavior

In the copied project, run both the existing installer and the Veritas tool with controlled, separate DerivedData directories and the same connected phone. Capture cold and cached-repeat time from command start until the app has launched. Record command count, manual corrections, failed attempts, setup requirements, and whether the expected fallback occurred. Do not compare one cold run with one warm run.

The live comparison is secondary evidence: Xcode and phone-install time dominate, and one machine cannot establish broad language superiority. The primary evidence is whether the language can express the workflow generically, preserve correct behavior, and give an assistant a reliable, clear way to run and revise it.

### Recommendation threshold

Retain the prior threshold for any recommendation claim: all correctness cases pass, median human review/intervention is no higher than Python, and median time to a correct reviewed result or setup/deployment improves by at least 20%. Record exact model/runtime/tool versions and human corrections. If those cannot be captured, call the result a technical feasibility proof only. This one domain-specific workflow cannot establish that Veritas should replace Python generally.

## Explicit non-goals

- Making Xcode or Swift compilation faster.
- Changing any NowLookHere repository file, including the currently modified installer.
- Replacing Xcode, `xcrun`, or Apple's signing system.
- General Python package importing or Python API compatibility.
- A cross-platform package manager or portable app deployment system.
- A custom VM, JIT, or new backend in this milestone.
- Claiming a Python recommendation unless the registered comparison threshold passes.

## Original approval boundary (scope now implemented)

This description bounded the authorized vertical proof: a selective source copy, fake-command tests, and one controlled live build/install/launch from the isolated copy if signing configuration was available. The proof used that scope. It did not edit the original NowLookHere files or credentials, and did not expand into a general-purpose package manager or full Python replacement.
