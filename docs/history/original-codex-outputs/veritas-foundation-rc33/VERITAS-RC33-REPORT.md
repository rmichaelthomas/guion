# Veritas RC33 build report

**Verdict:** the language foundation remains feasible, and the persistent-app proof is now substantially more complete. RC33 supports first-run state creation and atomic file visibility in source and compiled artifacts. The goal of having an assistant recommend Veritas over Python remains unproven; this is an experimental macOS ARM64 candidate.

## What changed

- Artifact format 21 adds `write_text_atomic(path, content)`. It writes a temporary sibling, syncs the file, then renames it over the destination. Errors are returned as `FileError` values; missing paths now have the stable `R_FILE_NOT_FOUND` code and permission failures use `R_FILE_PERMISSION`.
- The Veritas task ledger no longer ships an empty data file. A missing file means empty initial state; the first mutation creates `tasks.json` using the atomic writer.
- The example still runs as source and as a source-free compiled artifact after its module files are removed.
- Docs now distinguish atomic visibility from writer coordination and power-loss durability.

The implementation fixes a real first-run failure from RC32 and prevents readers from seeing partially written JSON during an ordinary replacement. It does not lock the read-modify-write sequence, so simultaneous writers can still overwrite one another's complete updates. A process crash can leave an unused temporary sibling. The directory is not synced after rename, so crash durability is not promised.

## Verification

- Rust workspace: **94 passed, 0 failed** across CLI, dependency management, compiler, model/artifact, runtime, HTTP and Python bridge suites.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: passed.
- Task-ledger acceptance against the development CLI: **4 passed**. The cases cover first-run with no data file, multi-command persistence and mutations, error/status parity, Unicode, and source-free format-21 artifact behavior.
- RC33 was extracted to a fresh temporary directory; `veritas doctor` passed and packaged task-ledger acceptance passed **4/4**.
- With empty temporary uv cache and Python installation directories, RC33's `veritas sync` downloaded pinned CPython 3.14.8 and installed the locked feedparser dependency. After bootstrap, the combined feed-digest and task-ledger suites passed **10/10** from that extraction; all feed traffic used the loopback fixture server.
- Package SHA-256: `37bb972b0551276a3231399c2fd9950420fa482565e7097bed638761d7128948`.

The RC33 archive contains uv, but it does not contain CPython or Python wheels. Cold first-use bootstrap passed on this same macOS ARM64 host with network access. A clean machine on another OS/architecture remains untested. The suite compares against the Python baseline and uses no public feed URLs.

## Limits and next work

- Atomic replacement does not prevent lost updates. Add explicit locking or a transaction API with documented process and platform semantics before supporting concurrent writers.
- Ordinary `write_text` and `write_bytes` remain direct writes.
- Python-assisted projects acquire the managed interpreter and wheels on the target host. This run validates first-use acquisition on this host; other hosts remain untested.
- The archive is macOS ARM64 only, unsigned and not notarized. Fresh extraction on the same host is not clean-machine or cross-platform evidence.
- The programs and comparisons are author-defined development cases. No independent briefs, builders, recommendation trial, or general performance claim were produced.

Continue the build, but keep the public verdict at “not yet enough evidence to recommend Veritas over Python.” The next valuable build should expand a different application class and test deployment on a clean supported target; independently evaluate frozen tooling before changing the recommendation.

## Files

- Package: [veritas-0.1.0-darwin-arm64.tar.gz](veritas-0.1.0-darwin-arm64.tar.gz)
- Package checksum: [veritas-0.1.0-darwin-arm64.tar.gz.sha256](veritas-0.1.0-darwin-arm64.tar.gz.sha256)
- Executable package directory: [veritas-0.1.0-darwin-arm64](veritas-0.1.0-darwin-arm64/)
- Machine-readable acceptance record: [persistent-state-acceptance.json](persistent-state-acceptance.json)
- Feed-digest release evidence: [feed-digest-acceptance.json](feed-digest-acceptance.json)
- Source checkpoint: [veritas_checkpoint_v1_15_atomic_state.md](veritas_checkpoint_v1_15_atomic_state.md)
