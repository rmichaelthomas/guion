# Changelog

Changes are recorded here as the project moves toward its first release. No public Guion release has been published.

## Unreleased — pre-release development

- Added a register form of the VM, translated per function on first call, which falls back exactly to the stack interpreter. `Map` is now a seeded hash table with shared keys; observable order stays sorted by key. `guion run` compiles only functions reachable from `main`, and the VM's local fuel reservation is 4096 instructions.
- Native functions that fail now keep the output they printed first; previously `print(7)` followed by a failure printed nothing. `print` of `Text` now compiles natively in functions with a loop. `csv_next_row` and `text_parse_integer` run as direct host operations, plain CSV records take a faster path, and the VM reads maps and lists without clones. The VM now polls cancellation at frame entry, loop edges and after host operations, so a cancelled program can finish its current straight-line run of instructions before stopping.
- Shrank the runtime `Value` from 88 to 24 bytes, added integer and clone fast paths to the VM, and made packages profile-guided by default (`GUION_PGO=0` opts out). Allocation limits charge the smaller value size, so a given `allocation_limit_bytes` now holds more values.
- Added format-45 `http_response_text` with bounded checked UTF-8 decoding, plus `std:http.http_response_json` for typed JSON parsing and stable error mapping. Existing HTTP request APIs retain their result types.
- Added format-41 stable integer-key sorting and merging plus `JsonSetLocal` bytecode for alias-safe, copy-on-write local JSON-object updates; added a fast artifact-format assertion audit that runs before long validation gates. Format 42 assigns distinct local-time opcode tags so temporal resolution coexists with those format-41 instructions.
- Added pure `std:time` local-time resolution that reports exact times, daylight-saving gaps, and folds with all candidate instants using bundled tzdata 2026c. Calendar events now preserve local endpoint identity and ambiguity instead of selecting an instant; `guion --version` reports the timezone-data version.
- Renamed active product surfaces from Veritas to Guion: CLI, Cargo packages, source and artifact extensions, project filenames, examples, standard library, compiler seed, and packaging tools.
- Added transition reads for selected legacy Veritas manifests, locks, project state, source files, and existing artifacts.
- Added a Guion-named acceptance-report cohort for nine retained applications; the frozen Veritas evidence remains unchanged.
- Replaced current-facing getting-started, architecture, reference, compatibility, and status documentation; preserved the Veritas-era checkpoints and results as history.
- Added an agent start guide, four runnable recipes, a versioned machine index, and CLI JSON report envelope schemas.
- Full Guion release-matrix, benchmark, evidence-index, and pre-release handoff checks remain open.
