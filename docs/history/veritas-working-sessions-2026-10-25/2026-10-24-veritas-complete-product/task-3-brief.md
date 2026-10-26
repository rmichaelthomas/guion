### Task 3 — Emit complete artifacts and make self-hosting operational

**Files:** new `language/self_host/artifact.vr`, driver/format scaffolding, `language/artifact-format.md`, bootstrap seed/manifest, `crates/bootstrap/`, `crates/cli/src/compiler_driver.rs`; modify model/CLI/runtime manifests, CLI command paths, self-host manifest, and existing bootstrap integration test.

**Consumes:** Task 2 checked lowering; current `Artifact::to_bytes`/reader/verifier contract.
**Produces:** `compiler_compile_project` and `compiler_assemble` returning full `.vrb` bytes; an installed self-hosted compiler; explicit developer-only stage-zero tooling.

- [ ] Document and implement encoding for every artifact field, value, type, instruction, source span, dependency identity and resource ceiling. Add bounded byte construction primitives if needed; do not add a host function that secretly performs complete artifact assembly.
- [ ] Cross-check emitted bytes with the native artifact reader and malformed-artifact cases (truncation, count/length overflow, bad opcodes, forged handles, invalid branches). Compiler artifacts must pass the same verifier as user artifacts.
- [ ] Bootstrap C1 from the existing Rust compiler; use C1 to compile the complete compiler to C2; use C2 to compile it to C3. Compare C2/C3 canonical artifact bytes after documented normalization of source-root metadata only. Do not remove actual instructions or diagnostics from equality checking.
- [ ] Use C2 and C3 to compile all conformance projects and complete known application graphs available at this stage. Repeat bootstrap after the final library/compiler additions; two handpicked programs no longer satisfy G1.
- [ ] Make the default CLI embed the checked compiler seed and invoke the Veritas compiler for check/run-source/build/test/doc/inspect. Keep stage zero in a separately named developer binary. Move `veritas-compiler` out of default runtime/CLI dependencies, retaining it only for bootstrap and differential tests.
- [ ] Preserve an acyclic seed-update workflow: build the explicit Rust stage-zero tool independently of the default CLI; use it to create the first compatible compiler artifact, then reproduce that artifact through Veritas stages before replacing the embedded seed. Increment artifact/host-ABI versions when required, retain documented old-artifact readers, and reject incompatible seeds rather than relying on a development path fallback.
- [ ] Replace the self-host manifest's smoke entry with the production compiler entry. Record deterministic source/seed/schema hashes and a reproducible regeneration command. Tampered or mismatched seed identity must fail startup/build clearly.
- [ ] Prove the installed compiler builds itself and user projects with stage-zero binaries and sources inaccessible. Add `default_cli_cannot_fallback_to_rust_source_compiler`; a dependency-graph inspection and a forced unsupported-input case must both confirm no fallback.

**Verify:** `cargo xtask bootstrap --stages 3` and `cargo xtask conformance --compilers stage2,stage3` pass. Checkpoint 1 reports the exact self-hosting boundary and any outstanding failures, then work continues.

**Pulled-forward dependency:** The first C2/C3 full-corpus run overflowed the normal host stack during guest-to-guest calls. Per the sequence rule below, Task 6's explicit guest-frame execution work is pulled forward to unblock this gate; the remaining Task 6 work stays in sequence.

