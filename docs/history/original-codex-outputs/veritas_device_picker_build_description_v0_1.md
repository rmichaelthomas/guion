# Veritas device-picker build description v0.1

**Date:** October 22, 2026  
**Status:** Proposed for approval; implementation has not started  
**Relationship:** Extends the Veritas Inception Checkpoint v1.0 and follows the measured [NowLookHere workflow baseline](veritas_nowlookhere_workflow_baseline_v0_1.md).  
**Purpose:** Test whether Veritas can make one useful assistant-written command-line task clearer and more reliable than its Python 3 equivalent.

## Intent

The north star remains: earn a point where a capable assistant would recommend Veritas over Python for a real builder task. This small test will not establish that broad recommendation. It will test whether Veritas can give an assistant a dependable path from a request to a working command-line tool, without the user correcting how to launch it.

Rob approved using the existing NowLookHere iPhone-install workflow as a real test context. The measured `scripts/install-device.sh` uses `python3` to read a JSON device list and choose a paired iPhone or iPad. Its actual build, install, and launch time is mostly Xcode and device work, so it is a poor first test of Veritas runtime speed. The device-selection step is a better first language task: it is real, bounded, and directly exercises the code Veritas would own.

## Proposed program

Build a small Veritas command-line program that reads a saved `devicectl`-style JSON file and selects a paired iPhone or iPad. With no requested name, it selects the first paired iPhone/iPad in the input order. With a `--device NAME` option, it selects the exact matching paired device. It prints the selected identifier and name in a stable format.

The Python baseline will implement the same behavior, using `python3` explicitly. Both programs will use the same synthetic JSON fixtures and expected output/error cases.

The intended Veritas invocation is:

```text
veritas run examples/select_device.vr --input tests/fixtures/devices.json [--device NAME]
```

The exact source-level JSON access syntax is open to implementation within the approved behavior. The implementation must provide general JSON values and checked access to objects, arrays, strings, and booleans; it must not hard-code device selection as a Veritas runtime builtin. The program's selection logic must be visible in Veritas source.

## Scope

The prototype will add only the language and CLI support needed for this task:

- program arguments after the source path, including `--input` and optional `--device`;
- reading the provided UTF-8 JSON file;
- parsing JSON into a general value representation usable by Veritas source;
- checking types and missing fields with useful diagnostics;
- deterministic text output and nonzero status for invalid input or no match.

Continue the current Rust-hosted compiler for this experiment so we can build on the existing parser, type checker, diagnostics, and CLI. This is an implementation choice for the next test only; Rust is not selected as Veritas's required user-facing runtime or permanent compiler architecture. The current Rust toolchain requirement must be counted as setup friction.

## Acceptance cases

Use synthetic fixture data only; do not save or publish real device identifiers. The program must:

1. Select the first paired iPhone when the input also includes an unpaired iPhone and a paired Apple Watch.
2. Select a paired iPad when it is the first eligible iPhone/iPad in input order.
3. Select an exact named device when `--device NAME` is supplied.
4. Reject a requested name that is absent or only present on an unpaired device, with a clear message and no success output.
5. Reject a list with no paired iPhone or iPad with a clear message.
6. Reject malformed JSON and malformed/missing required fields without panicking or silently selecting a different device.
7. Produce byte-for-byte stable output for the same input.

The Python and Veritas versions must pass the same cases. The Veritas version must run with the documented `veritas run ...` command, without asking the user to install or invoke Python. The Python baseline command must use `python3`.

## Evaluation

Use the prior comparison method where feasible: clean workspaces, identical prompt and fixtures, a base implementation task followed by one small change (for example, prefer a named device), shared acceptance tests, independent sessions, and recorded setup time, total time, failed attempts, human corrections, diagnostics, and follow-up change cost. If exact model/version metadata or review time cannot be captured, report the result as inconclusive rather than claiming a speed win.

Retain the previously registered threshold for any recommendation claim: all correctness cases pass; median human review/intervention is no higher than Python; and median time to a correct reviewed result or setup/deployment improves by at least 20%. This single utility may at most support a narrow CLI recommendation; it cannot establish that Veritas should replace Python generally.

## Boundaries

- This task consumes a saved fixture. It does not call `xcrun`, build the iOS app, install anything, launch anything on a phone, or change the NowLookHere repository.
- It does not try to make Xcode compile faster. The observed phone-install path stays a later integration benchmark.
- No package manager, arbitrary Python imports, general subprocess API, C FFI, WebAssembly target, or general iOS tooling is added in this milestone.
- Do not claim that a native Veritas executable or “one binary” user experience exists; this prototype's `veritas` compiler still requires Rust to build from source.

## Decision requested

Approve or revise this bounded device-picker experiment before implementation. If approved, the next step is to turn this description into an implementation plan, then build and test only this scope.
