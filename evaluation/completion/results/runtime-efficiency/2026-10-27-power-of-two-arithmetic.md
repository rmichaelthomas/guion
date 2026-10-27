# Positive power-of-two arithmetic microbenchmark

Date: 2026-10-27
Branch: `codex/guion-richards-gap-closure`
Host: macOS ARM64; Python reference: CPython 3.14.7

## Workload

The fixture loops over 25,000 negative-to-zero integers. Each iteration performs floor division and remainder by 1, 4, 8, and 16, then accumulates the results. The Python and Guion versions produce the same output: `-448956220`. This is a deliberately narrow arithmetic microbenchmark, not an application benchmark.

## Results

| Lane | Previous Guion | Candidate Guion | CPython 3.14.7 | Candidate / previous (paired 95% interval) |
| --- | ---: | ---: | ---: | ---: |
| Fresh source | 8.980 ms | 8.206 ms | 32.943 ms | 0.898 (0.861–0.916) |
| 25 executions per process | 1.102 ms per execution | 0.515 ms per execution | 5.937 ms per execution | 0.468 (0.458–0.474) |

Candidate/Python was 0.246 (paired 95% interval 0.239–0.253) for fresh source and 0.0865 (0.0853–0.0879) for the repeated lane. Two of 30 fresh-source pairs tied or regressed; none did in the repeated lane.

Generated native code for the fresh-source run fell from 58 to 46 Cranelift IR blocks and from 655 to 482 instructions. For the repeated wrapper, it fell from 85 to 73 blocks and from 965 to 792 instructions. Neither candidate lane deoptimized.

## Method and limits

Both lanes used randomized paired order, 30 trials per arm, two warmups, and an output check on every run. Guion's source cache was bypassed. The OS file cache was uncontrolled. The Python repeated lane loads its module once and runs the same calculation 25 times in one process. These results establish a gain for this focused arithmetic loop; they do not predict the speed of arbitrary Guion programs.

## Verification

The compiler specializes positive power-of-two literals for floor division and remainder. The native regression checks divisors 1, 4, 8, and 2^62 against hand-checked outputs and the VM for negative values and signed 64-bit boundaries, compares generated instruction counts against dynamic divisors, and keeps the existing divisor-two regression green. The runtime crate suite passed 175 tests, formatting passed, and the locked offline release build passed with two Cargo jobs.

The [Guion fixture](../../performance/fixtures/power-of-two-arithmetic/main.gu), [Python reference](../../performance/fixtures/power-of-two-arithmetic/reference.py), and [machine-readable comparison](2026-10-27-power-of-two-arithmetic-30-trial-comparison.json) are included for reuse.
