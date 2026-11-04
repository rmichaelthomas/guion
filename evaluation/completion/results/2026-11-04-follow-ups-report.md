# Follow-ups to Wave 1 and Track A

Date: 2026-11-04. Branch `feat/follow-ups`, artifact format 61. Each open item from the Wave 1 report, the Track A report and the build summaries was first checked against the repository; this report says what was already done, what was built, and what remains.

## Already done before this pass

- **Int→float and number→text conversion.** `int_to_float`, `float_to_int`, `int_to_text` and `float_to_text` landed in B1 (format 52, d13a955) and are in `language/spec.md` and the agent reference. Checked by running them.
- **Default registry address** and **registry-to-registry dependencies** were closed in b639aa3.

## Built in this pass

| Item | What changed | Evidence |
|---|---|---|
| String building hit the allocation budget at about 60 KB | `set s = s + a + b` parsed as `(s + a) + b`, so the first addition copied `s` every time. Both compilers now regroup it as `s + (a + b)` for Text locals (text concatenation is associative, operands still evaluate in order, type errors are reported against the code as written), so the VM's in-place append applies. Wrapping text (`"<" + s + ">"`) still copies the whole string each time and is charged for it. | `chained_text_append_to_a_local_stays_linear_under_the_allocation_budget` (fails without the fix) |
| `random` on the machine-code path | Five format-61 instructions (`RandomSeed`, `RandomAdvance`, `RandomBits`, `RandomUnitFloat`, `RandomBelow`) keep the SplitMix64 state as a signed 64-bit pattern, so the native backend runs them on plain registers. Sequences are bit-identical to format 52 for every seed tried (negative, 2^64−1, 2^64, 10^23). Added `random_next_i64`. A range wider than 2^64 values used to loop forever; it now raises `R_RANDOM_RANGE`. | `random_sequences_are_unchanged_by_the_format_61_generator`, `random_draws_and_unit_functions_compile_natively_and_match_the_vm` |
| `guion update` ignored newer registry versions | `guion add NAME` records `registry = "<range>"` on the dependency (a caret on the chosen version by default). `guion update` resolves that range again, with the same index, log and byte checks, keeps the installed version eligible if it was yanked, and still refuses a capability widening without `--approve-capabilities`. | `update_moves_a_registry_package_to_the_newest_release_its_range_allows` |
| No token revocation | Worker: `POST /v1/tokens/rotate`, `POST /v1/tokens/revoke` (publisher) and `POST /v1/admin/publishers/<id>/revoke` (administrator; also finds tokens issued before this change). Client: `guion token rotate` and `guion token revoke`. | 2 new Worker tests; `token_rotate_and_revoke_retire_the_old_token` |
| Network hosts not verified | `guion check` and `guion publish` compare declared `network` hosts with hosts in URL literals in the library's own functions: an undeclared literal host is an error; network use with no literal host at all warns that the declaration cannot be confirmed. Hosts built at run time remain unverifiable statically. | `hosts_named_in_the_code_must_be_declared` |
| `remove`/`rename` followed symlinks | They now act on the named entry: removing or renaming a link removes or moves the link, and a link pointing outside the root can be removed. | `symlinks_are_removed_and_renamed_as_entries_and_never_written_through_when_dangling` |
| TLS client certificates | `tls_connect_with_identity` presents a client certificate; `tls_accept_verified` requires one and verifies it against a given CA only (format 61). | `mutual_tls_accepts_...`, `mutual_tls_rejects_missing_and_untrusted_client_certificates` |

## Defects found and fixed along the way

- **Sandbox escape through a dangling symlink.** `write_text("link", ...)` where `link` pointed at a missing file outside the root created that file outside the root, because the resolver treated a non-existent target as a new file in the root. Writes through a dangling symlink are now refused with `R_HOST_DENIED`. Reproduced before the fix; covered by the symlink test above.
- **Functions returning `Unit` never ran as machine code**, including `fn main() -> Unit:`. Every such program ran its main loop in the VM. They now compile natively.
- **`assert` blocked native compilation** of any function that used it. It now compiles; a failed assertion replays in the VM, which raises `R_ASSERTION`.
- **Native code deoptimized on every call that built a record holding another record** (for example `RandomFloat(value, Rng(state))`), because the native-layout inner record was passed to the bridge as an untagged pointer. It is now tagged and decoded; deoptimizations in the random benchmark went from 2,000,000 to 0.
- **`docs/self-hosted-compiler.md` was wrong**: the CLI compiles with the Rust `guion-compiler` crate (since 7a909f7), not the embedded seed. Corrected; code-generation changes go into both compilers.
- The registry end-to-end tests each start `wrangler dev`; run in parallel on a busy machine they could miss the start-up wait. They now take turns.

## Measurements (macOS ARM64, best of three warm runs)

| Benchmark | Before | After | CPython 3 |
|---|---|---|---|
| 1,000,000 iterations of `random_next_float` + `random_next_range` | 5.57 s | 1.17 s | 0.32 s |
| 20,000,000-iteration integer loop in `fn main() -> Unit` | 0.61 s | 0.11 s | 1.36 s |
| 200,000 chained appends (`text + int_to_text(i) + ","`, 1.29 MB) | fails with `R_ALLOCATION_LIMIT` at about 60 KB | 0.056 s | 5.04 s |

The random benchmark needs `instruction_limit` and `allocation_limit_bytes` raised above their defaults (see open items).

## Verification

Full workspace: 659 tests passed, 0 failed (registry end-to-end tests also run serially: 4 passed). Clippy with warnings denied is clean. Three-stage bootstrap and conformance (stage2, stage3) pass. Registry Worker tests: 11 passed. Site build, site check and 112 site tests pass. Not run: Linux, and the `verify-wave-1.sh` / `verify-package-registry.sh` table refresh.

## Still open

- **Nested records on the machine-code path.** The remaining gap to CPython in the random benchmark is native code handing records that contain records through the VM bridge. Giving nested native-layout records a native layout is a general backend feature.
- **Default runtime limits.** A plain `guion run` allows 2,000,000 instructions and 256 MiB of *cumulative* allocation, so a long loop that allocates small records stops even when its live memory is tiny. Changing either is a product decision.
- **`guion run` grants read and write on the project root by default.** A policy decision.
- **Signing.** The protocol reserves a `signature` field; who signs (publishers, the registry, or both) is a design decision.
- **`.gub` cache attachments.** Nothing can consume a precompiled library: the compiler links whole programs from source. Attachments only make sense after separate compilation exists.
- **Deployment.** The Worker with the token endpoints is not deployed; the live registry still runs the earlier version.
- Socket hostname grants still match the name as written, before resolution; HTTP/2, `Expect: 100-continue`, ALPN and session-resumption control are not offered; TLS hostname mismatch is not tested against a real mismatching peer; Linux results for Wave 1 are not established.
