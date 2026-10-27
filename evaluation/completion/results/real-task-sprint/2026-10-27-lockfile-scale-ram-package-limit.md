# Lockfile auditor RAM and scale result — 2026-10-27

## Result

The lockfile auditor now processes **16,384 packages and writes the exact sorted diagnostic report**. Its peak RAM is below Python at both tested sizes. The change applies an app-specific 5,000,000-instruction ceiling; Guion's global runtime default is unchanged.

The memory result closes the measured gap for this workload. The speed result is mixed: Guion is near Python at 8,192 packages, but remains slower at 16,384.

## Measurements

Seven fresh-process timing pairs and seven output-checked peak-RSS runs were collected for each size on one Apple Silicon Mac. Guion source ran with its source cache bypassed. The harness checked every report against the expected JSON. OS file-cache state was uncontrolled.

| Packages | Guion median | Python median | Paired timing ratio (Guion/Python) | Guion peak RSS | Python peak RSS | RAM change |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8,192 | 0.143 s | 0.154 s | 0.952× (95% interval 0.677–1.163) | 27.70 MiB | 40.38 MiB | 31.4% lower |
| 16,384 | 0.254 s | 0.178 s | 1.416× (95% interval 1.377–1.430) | 44.66 MiB | 47.86 MiB | 6.7% lower |

At 8,192 packages, the timing interval includes parity, so the small median speed advantage is not a conclusive speed win. At 16,384, Guion is about 42% slower, while using about 3.2 MiB less peak RAM. The prior implementation used 49.64 MiB at 8,192 packages; the new version uses 27.70 MiB, a 44.2% reduction.

The 16,384-package fixture contains 32,768 diagnostics and produces a 1,487,359-byte report. Both the 8,192- and 16,384-package acceptance checks passed with exact output.

An output-checked 8,192-package control with findings disabled used 16.83 MiB in Guion and 34.68 MiB in Python (seven runs each). This confirms that parsing and validating the input are not the source of Guion's former higher peak; retaining all report fragments at once caused the excess. The control uses the same final Guion source snapshot as the scale benchmark; raw control samples are in [`2026-10-27-lockfile-input-only-memory-control.json`](2026-10-27-lockfile-input-only-memory-control.json).

## What changed

- Added a native, linear `text_join_native` primitive and routed `std:text.text_join` through it. It checks collection/text ceilings and charges the joined output to the guest allocation budget.
- The lockfile auditor serializes diagnostic rows in batches of 128. This releases row fragments between batches instead of retaining all 32,768 fragments together.
- Raised only the lockfile auditor's configured instruction ceiling to 5,000,000 so the 16,384-package case completes. The global default remains unchanged.
- Added artifact opcode tag 163 and format 43, with checks that reject the new operation in older artifact formats. Regenerated the self-host compiler seed and documented the format change.

## Verification

- Release CLI build passed.
- Model unit tests passed: 18/18.
- Standard-library data tests passed: 22/22, including empty joins, Unicode delimiters, both VM modes, artifact round-trip recognition, and rejection under format 42.
- Three-stage self-host compiler equality passed: 1/1 in 325.39 seconds. The regenerated compiler artifact produced identical C2 and C3 bytes.
- Lockfile scale tests passed for exact sorted output at both package counts.
- The full seven-run benchmark and its raw samples are in [`2026-10-27-lockfile-scale-native-text-join.json`](2026-10-27-lockfile-scale-native-text-join.json).

The workspace-wide formatting check reports existing formatting differences in `crates/compiler/tests/pipeline.rs`, `crates/runtime/src/lib.rs`, and pre-existing lines in `crates/model/src/lib.rs`; no unrelated formatting changes were retained.

## Scope

This closes the lockfile auditor's measured RAM gap and doubles its verified package scale from 8,192 to 16,384. It does not establish that Guion is generally faster than Python; the 16,384-package timing is still slower, and these results cover one application workload on one host.
