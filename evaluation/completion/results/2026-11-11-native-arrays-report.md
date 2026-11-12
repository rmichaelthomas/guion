# Native arrays report

Date: 2026-11-11. Branch `feat/native-arrays`. Artifact format 62. Verification table: `arrays-verification.md` (produced by `scripts/verify-arrays.sh`).

## What landed

| Task | Commit | Contents |
|---|---|---|
| 1 | a866576 | Array values, eleven element types, `Op::Array`, artifact format 62 (type tag 18, op tags 193 and 194) |
| 2–3 | 2c13156 | `Array[e]` in the Rust compiler; catalog-driven array builtins; construction, access, shape and byte interchange; in-place `set a = array_set(a, i, v)` |
| 4–5 | 8c6231b | Broadcasting arithmetic, comparisons, float math, exact reductions, axis reductions, stable sort, `dot`, `matmul`, ranges, `linspace`, seeded random and normal arrays |
| 6 | 60771e3 | The self-hosted compiler types and encodes arrays; the two compilers accept, reject and compile array programs identically |
| 7 | 1168a1d | Machine-code interaction test; benchmarks against NumPy and plain CPython |

The language contract is in `language/spec.md` (the "Native arrays" bullet under Runtime rules).

## Machine code

Array functions run in the VM. The machine-code tier compiles the functions around them: in the test program, an integer loop called from an array function compiles to machine code, the array functions are listed as VM-run with a reason, and output is identical with machine code on and off (`crates/runtime/tests/arrays_native.rs`). No runtime change was needed; array instructions already fall back to the VM.

## Speed

One Apple M1 Pro, macOS 27.0.1, release build, NumPy 2.5.1, CPython 3.14.7. Each lane ran 15 times, interleaved in shuffled order, timed end to end (process start to exit, including Guion's compile and the NumPy import). Every output matched NumPy's to a relative tolerance of 1e-9. Ratios below 1 mean Guion took less time. Raw samples: `evaluation/development/arrays/results.json` and `results_large.json`.

### At the plan's sizes, against NumPy

| Workload | Guion (s) | NumPy (s) | Guion / NumPy (95% interval) |
|---|---|---|---|
| `print(0)` (start-up only) | 0.0151 | 0.0668 | 0.223 (0.213–0.237) |
| `a*b+c` on 10M f64, 5 passes | 0.2209 | 0.1643 | **1.330** (1.288–1.353) |
| sum of 10M f64, 10 passes | 0.0599 | 0.1022 | 0.588 (0.575–0.614) |
| 512×512 f64 matmul | 0.0421 | 0.0679 | 0.623 (0.596–0.637) |
| stable sort of 1M f64 | 0.0499 | 0.0874 | 0.572 (0.551–0.588) |
| guest loop of 1M `array_get` | 0.0886 | 0.2048 | 0.428 (0.424–0.438) |

These end-to-end ratios flatter Guion: NumPy spends about 52 ms more than Guion just starting (importing NumPy), which is most of its time on several rows. Subtracting each side's start-up median gives a rough estimate of the compute alone:

| Workload | Guion compute | NumPy compute | Estimate |
|---|---|---|---|
| `a*b+c` | ~206 ms | ~98 ms | Guion about 2.1× slower |
| sum | ~45 ms | ~35 ms | Guion about 1.3× slower |
| matmul | ~27 ms | ~1 ms | Guion about 25× slower (NumPy uses the platform BLAS; Guion uses a blocked loop) |
| sort | ~35 ms | ~21 ms | Guion about 1.7× slower |
| `array_get` loop | ~74 ms | ~138 ms | Guion about 1.9× faster (NumPy's per-element indexing is slow) |

So for short scripts Guion finishes first, and for bulk numeric work NumPy's kernels are faster, by a wide margin for matrix multiplication.

### At smaller sizes, against NumPy and plain CPython lists

4M-element vectors and a 256×256 matmul, so the plain-CPython lane finishes in reasonable time.

| Workload | Guion (s) | NumPy (s) | CPython (s) | Guion / NumPy | Guion / CPython |
|---|---|---|---|---|---|
| start-up only | 0.0150 | 0.0673 | 0.0235 | 0.219 (0.209–0.283) | 0.641 (0.589–0.831) |
| `a*b+c`, 5 passes | 0.0899 | 0.1032 | 1.5848 | 0.869 (0.825–0.914) | 0.057 (0.056–0.059) |
| sum, 10 passes | 0.0370 | 0.0784 | 0.3214 | 0.465 (0.436–0.496) | 0.116 (0.107–0.125) |
| 256×256 matmul | 0.0188 | 0.0655 | 0.5962 | 0.278 (0.263–0.312) | 0.032 (0.029–0.035) |
| sort of 1M | 0.0501 | 0.0877 | 0.1865 | 0.578 (0.558–0.597) | 0.268 (0.264–0.287) |
| 1M `array_get` loop | 0.0891 | 0.2059 | 0.1342 | 0.432 (0.428–0.457) | 0.660 (0.646–0.665) |

Against plain CPython lists Guion is faster on every workload, from 1.5× (the indexing loop) to 31× (matmul).

## Limits found

- **Memory limit.** The 10M-element `a*b+c` run needs about 486 MB (peak RSS), more than the 256 MiB default `allocation_limit_bytes`, and fails with `R_ALLOCATION_LIMIT` unless the project raises it. The large benchmark set raises it to 2 GiB in its `guion.toml`. NumPy has no such limit.
- **Elementwise kernels** are about twice NumPy's cost per element. Each operation allocates a fresh result and checks every float result for finiteness; there is no fused or in-place form.
- **Matrix multiplication** has no BLAS.
- **Self-hosted compiler** emits plain `ArraySet` rather than the in-place `ArraySetLocal`, as it already does for `list_set`. Programs it compiles produce the same output, but an `array_set` loop copies on each pass.
- Not provided: strided views, fancy indexing, complex numbers, operator syntax on arrays, and the Python bridge for arrays.
