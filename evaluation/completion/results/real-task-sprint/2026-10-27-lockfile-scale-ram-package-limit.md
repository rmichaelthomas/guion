# Lockfile auditor speed, RAM, and scale result — 2026-10-27

## Result

The lockfile auditor now passes its exact-output checks through **16,384 packages and 32,768 diagnostics**, while using less peak RAM than Python. The latest run also closes the measured speed gap for this workload: Guion is slightly faster than Python at 16,384 packages and substantially faster at 4,096 and 8,192.

This is a confirmed win for this lockfile workload on this Mac. It is not evidence that Guion is generally faster than Python.

## Latest measurements

Seven randomized fresh-process timing pairs and seven output-checked peak-RSS samples per arm were collected at each size. Guion's source cache was bypassed and fixture setup was excluded. The benchmark checks every generated report against the exact expected sorted JSON.

| Packages | Guion median | Python median | Guion/Python time ratio (95% interval) | Guion/Python peak RSS | Versus previous committed implementation (`ed06a82`) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 4,096 | 56.35 ms | 117.45 ms | 0.479× (0.469–0.490) | 18.55 / 36.14 MiB | 0.750× time |
| 8,192 | 95.29 ms | 137.88 ms | 0.692× (0.676–0.700) | 26.16 / 40.44 MiB | 0.717× time |
| 16,384 | 171.82 ms | 179.87 ms | 0.948× (0.942–0.975) | 40.92 / 47.13 MiB | 0.699× time |

A ratio below 1 means Guion took less time. At 16,384 packages, Guion used about **13% less peak RAM** and took about **5% less time** than Python. The 95% interval stays below parity. Against the previous committed implementation, the new run takes about **30% less time** at that scale.

The earlier scale run had Guion at 1.416× Python for 16,384 packages. The new native report serializer changes that result to 0.948×. Its full raw timings, memory samples, run order, platform details, and output-verification flags are in [`2026-10-27-lockfile-native-pair-serialization.json`](2026-10-27-lockfile-native-pair-serialization.json).

The frozen lockfile auditor acceptance suite also passes **17/17 cases in Guion and 17/17 in Python**, including malformed-input handling and preservation of existing output on failure. The exact results are in [`2026-10-27-lockfile-native-pair-acceptance.json`](2026-10-27-lockfile-native-pair-acceptance.json).

## What changed

- Added `json_stringify_pairs`, a standard-library operation that streams `Map[Text, List[Text]]` into a sorted JSON array of two-field text objects. It preserves list order, escapes names and values, and checks collection, text, and allocation limits before creating the output.
- Added artifact opcode tag 164 and artifact format 44, updated the self-hosted compiler's opcode table, and regenerated its embedded compiler seed.
- Replaced the auditor's interpreted per-row JSON encoding and chunk assembly with one call to `json_stringify_pairs`.
- Added direct Serde serialization for shared JSON values, avoiding a cloned intermediate `serde_json::Value` for ordinary `json_stringify` calls.
- Kept the auditor's app-specific 5,000,000-instruction limit. Guion's global runtime limit remains unchanged.

## Verification

- Release CLI build passed; the build confirmed the regenerated embedded compiler seed matches its source.
- The generated builtin catalog and artifact-format audit passed.
- Model direct-serialization test passed, and all **17 JSON-focused runtime tests** passed.
- All **17 frozen lockfile cases** passed in each language.
- The 4,096-, 8,192-, and 16,384-package timing and memory runs passed with exact output checks.

## Measurement limits

These are fresh-process source runs on one Apple Silicon host, using Python 3.14.7. OS file-cache state was uncontrolled. They establish the lockfile auditor's speed, memory, and scale behavior on this workload; other programs, hosts, and cold-cache conditions need their own measurements.
