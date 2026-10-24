# Veritas checkpoint v1.21: package integrity checks

**Status:** active build checkpoint. This advances the approved north-star work; it does not establish that an assistant should recommend Veritas over Python.

## Objective

Make the hashes in current-host package reports useful at the point of use. Reject damaged packages before application code starts, and keep valid managed-Python packages stable across repeated launches.

## Implemented

- Added `veritas verify-package DIRECTORY`. It checks the current host target and the report's SHA-256 identities for the source-free artifact, Veritas runtime, declared resources, managed CPython tree, pinned `uv`, and Python dependency lock. It rejects unsafe resource paths and files that resolve outside the package.
- The generated `./run` launcher verifies the package before every application invocation and emits no success text into the app's output. Failure is nonzero and reaches the user's terminal.
- `PACKAGE.txt` documents the verifier and says the unsigned report detects corruption but does not authenticate the publisher.
- Added corruption tests for a declared data resource and a file inside the bundled Python runtime. They verify that package launch fails before app output appears.
- The repeated-launch check exposed that CPython writes `.pyc` files into its own bundled runtime, which invalidated the tree hash. The supervised sidecar now uses Python's `-B` option to avoid mutating that runtime. The moved feed-inbox and native-wheel packages pass verification before repeated offline launches.
- Expanded release assembly to include evaluation briefs, raw JSON results, runnable Python acceptance tests, the known-workload runner, and the latest checkpoint in the archive.

## Evidence

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline` with pinned `VERITAS_UV`: 105 passed, 0 failed. Package tests include valid verification, resource corruption, Python runtime corruption, relocation into paths with spaces/Unicode, and repeated offline runs with `feedparser`, `orjson`, and NumPy.
- `python3 -m unittest discover -s evaluation/tests -v` with the rebuilt CLI and pinned toolchain: 20 passed, 0 failed.
- RC40 release archive's checksum, extracted `doctor`, and current-host native-wheel package smoke test were verified. The release build that includes this checkpoint and verifier is the next output archive.

## Limits and open risks

- Hashes are unsigned and can be changed together with package contents; publisher authentication is outside this unsigned local release candidate.
- The verifier does not hash the offline uv cache or generated virtual environment. The locked offline `uv sync` runs before Python calls, but that is not a complete integrity proof for every installed dependency file.
- This host's test covers two compiled wheels and their observed macOS system-library links; it is not a general native-library closure audit or Python license inventory.
- Only macOS ARM64 has package relocation evidence. Other operating systems and clean-machine installers remain unverified.
- Known workload tests are author-run. No independent assistant-choice or builder study supports recommending Veritas over Python yet.

## Next north-star work

Complete dependency provenance and legal notices for Python packages, verify or reconstruct the managed installed environment without trusting mutable generated files, and validate archive install behavior. Then freeze the toolchain and run the preregistered independent task set. Keep the recommendation gate open until those observations support a task-specific choice.
