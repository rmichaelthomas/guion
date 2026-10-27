# Browser ABI implementation and release plan

- **Branch:** `codex/guion-browser-abi-contract`
- **Base:** merged PR #7, `7da43cbbc0e692b903cc04878e2a4ef248d1dadb`
- **Repository owner:** `rmichaelthomas/guion`
- **Status:** v1.0 code, docs, schemas, fixtures, and local tests are implemented and green in this branch. Parent task owns pull request, merge, and branch cleanup.

## Completed in this branch

1. Added `guion_wasm_abi_version()` with packed ABI major/minor semantics; reports 1.0 while retaining all legacy exports.
2. Added strict typed request/response execution and pointer/length result exports on top of `PortableHost` and the shared VM.
3. Added exact wire conversion for Int, Float64, Bool, Text, Bytes, Unit, List, Map, Record, Option, Result, and Json. Arbitrary integers remain decimal strings; floats retain exact finite IEEE-754 bits; records preserve declared fields; maps require sorted unique keys.
4. Added input/response/wire/runtime limits, bounded input allocation tracking, stable ABI error codes, no capability grants by default, and explicit rejection of non-transferable runtime values.
5. Added native regression coverage, schemas, golden fixtures, a runnable `.gu` sample, an actual-Wasm Node smoke script, and root README links.
6. Confirmed tests were added before implementation and observed failing on the missing v1 export; after the implementation, targeted tests pass.

## Verification gates

- [x] Native adapter tests cover the supported typed values, typed text and arbitrary integer calls, malformed requests, strict fields, noncanonical input, unsupported resources, capability denial, depth/output limits, and legacy behavior.
- [x] `cargo fmt --all -- --check`.
- [x] `cargo test --workspace --release --locked --offline` (after allowing the existing loopback fixture).
- [x] `cargo clippy --workspace --all-targets --release --locked --offline -- -D warnings`.
- [x] `cargo build -p guion-vm-wasm --target wasm32-unknown-unknown --release --locked --offline`.
- [x] Inspect exports/imports from the compiled module: required legacy and v1 exports exist, imports are empty.
- [x] Run the checked-in legacy and v1 calls from the Node Wasm smoke against the built module.
- [ ] Run a browser-engine smoke against the same module when the browser runner is available.
- [ ] Review final diff, commit the branch, and open one Guion PR.

The items above are release gates, not user-visible partial milestones. The AllHere native library remains independent of this ABI and can integrate the optional computation runtime only after these export/schema contracts are verified.

## Deliberately deferred

- Browser-side source compilation remains in AllHere's existing adapter. Generalize it only after a second independent consumer is identified.
- Event, document-state, and persistence capabilities have no demonstrated cross-project contract yet. ABI 1.0 rejects all requested capabilities.
- A browser-engine check remains open: no Playwright/Puppeteer package is installed, the installed Chrome binary aborts in headless mode with exit 134 before page output, and the app browser policy rejected a `file://` smoke page and disallows alternate routes around that restriction. The Node runtime smoke executes the actual compiled module and checks its no-import policy; do not describe it as a Chromium/Safari check.
