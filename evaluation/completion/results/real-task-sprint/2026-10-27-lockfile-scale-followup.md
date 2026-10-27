# Lockfile auditor scale follow-up — 2026-10-27

## Result

The lockfile auditor now completes the 8,192-package fixture and writes the exact expected report containing 16,384 sorted findings. The merged baseline failed this same case with `R_STEP_LIMIT` at Guion's 2,000,000-instruction default.

The lowest passing configured ceiling found by a boundary probe was 2,154,912 instructions. The application's `veritas.toml` now sets 2,250,000, leaving about 95,000 instructions of headroom. This is an application-level limit; Guion's global default is unchanged. No runtime or compiler code changed in this pass.

## Speed and memory

Seven randomized, output-checked fresh-process pairs compared uncached Guion source with the Python reference. Fixture generation was outside the timed interval, and OS file-cache state was uncontrolled.

| Packages | Guion median | Python median | Paired Guion/Python time | Guion peak RSS | Python peak RSS |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 4,096 | 76.2 ms | 114.5 ms | 0.666 (95% interval 0.653–0.670) | 31.3 MB | 38.0 MB |
| 8,192 | 131.1 ms | 136.3 ms | 0.974 (95% interval 0.960–0.992) | 51.2 MB | 42.2 MB |

At 4,096 packages, the candidate's timing remained statistically consistent with the pre-follow-up build (candidate/baseline ratio 0.992; 95% interval 0.976–0.996). At 8,192, Guion was about 2.6% faster by the paired median, while using about 21% more peak memory than Python.

## Verification

- The new 8,192-package regression test passed with exact sorted-output comparison.
- Lockfile application acceptance passed all 17 cases in Guion and all 17 in Python.
- The performance harness unit suite passed all 34 tests.
- The model artifact compatibility test passed.
- The release CLI build and calendar source check had already passed on the merged baseline before these manifest, harness, and documentation changes; no runtime or compiler implementation code changed.

The current evidence closes the 8,192-package correctness and budget boundary. It does not establish performance or memory behavior above 8,192 packages, and the memory gap remains a concrete follow-up target.

## Evidence

- [Raw timing and peak-memory samples](2026-10-27-lockfile-scale-postmerge-fix.json)
- [Lockfile acceptance results](2026-10-27-lockfile-acceptance-postmerge-fix.json)
- [Repeatable 8,192-package regression test](../../performance/test_lockfile_scale.py)
- [Previous scale checkpoint](2026-10-27-lockfile-auditor-scale.md)

## Source allocation follow-up — 2026-10-27

An untimed Guion runtime profile showed that the source created a placeholder `SemVer` record for every package before replacing it with a cached parse. The lockfile auditor now parses only on a cache miss and reads the cached value for each package. This is an application-level change; the language, compiler, and runtime are unchanged.

The post-change profile produced the exact expected report at 4,096 and 8,192 packages. Estimated guest allocation-budget charges fell from **19.11 MB to 15.72 MB** at 4,096 and from **38.21 MB to 31.43 MB** at 8,192 (about **18% lower** at both sizes). Per-package record-charge events fell from 4,096/8,192 to **zero**. The profile reported no list-copy bytes before or after. These counters estimate Guion budget charges; they are not host allocator totals or RSS measurements. [Before profile](2026-10-27-lockfile-guest-allocation-profile.json) and [after profile](2026-10-27-lockfile-guest-allocation-profile-post-optimization.json).

Seven output-checked memory samples did **not** show a host peak-RSS reduction. At 4,096 packages, median Guion peak RSS was **32.60 MB** after the change versus **31.26 MB** before it (+4.3%); at 8,192 it was **51.97 MB** versus **50.89 MB** (+2.1%). At 8,192, Python measured **42.21 MB**. These are medians of seven child-process peak-RSS observations, not process-tree totals. The timing comparison was inconclusive against the prior Guion source at both sizes: candidate/baseline ratios were **0.968** (95% interval **0.752–1.008**) at 4,096 and **0.978** (**0.958–1.012**) at 8,192. The latest 8,192 candidate/Python ratio was **0.967** (**0.941–0.979**), or about 3.3% less elapsed time for Guion. Every timed and memory output matched the exact report. [Seven-pair timing and memory confirmation](2026-10-27-lockfile-scale-memory-optimization-confirmation.json).

The post-change 16,384-package screen still exceeds the app's **2,250,000-instruction** ceiling: both the previous source and the candidate returned `R_STEP_LIMIT`, while Python completed the exact 32,768-finding report in **219 ms**. This is a capacity boundary, not a failed output comparison. No timing cohort ran after the Guion preflight failed. [16,384-package screen](2026-10-27-lockfile-scale-16384-post-optimization.json).

This pass removed the measured per-package placeholder record and reduced guest-budget charges, but it did not reduce host peak RSS or establish the source of the remaining RSS gap. The 16,384-package fixture also remains above the current app-level instruction ceiling. The results do not yet justify a wider compiler/runtime change; a later memory pass would need host-allocation attribution or a controlled report-size experiment to distinguish runtime overhead from the materialized output.
