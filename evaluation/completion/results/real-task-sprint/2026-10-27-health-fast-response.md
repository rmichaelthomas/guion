# Health-Digest Fast-Response Diagnostic — 2026-10-27

## What changed in the diagnostic

The frozen health-digest case includes a slow endpoint configured to respond after 700 ms, while the client timeout is 500 ms. That wait largely determines the total duration. To separate response-wait time from program work, this diagnostic removes that one fixture delay and changes the expected response from `timeout` to `healthy`. The other three endpoints, concurrency checks, output checks, and the task implementations remain unchanged.

## Results

Seven randomized paired trials with output checks measured a Guion median of **33.8 ms** and a Python median of **127.7 ms**. The median paired Guion/Python ratio was **0.260**, with a 95% bootstrap interval of **0.256–0.305**. Both implementations passed all checks in every trial.

The original frozen case measured a ratio of **0.839** on commit `6004e9a`. The fast-response diagnostic shows that most of that near-parity result came from the deliberate network wait. This is useful end-to-end evidence, but it does not make health-digest a high-value runtime optimization target.

## Method and limits

The diagnostic uses the same source programs and local HTTP fixture server as the frozen case, with only the `/slow` delay removed and that response's expected status adjusted. It is not part of the frozen acceptance suite. Timings include fresh process execution and output checks, exclude fixture setup, bypass Guion's source cache, and leave the OS file cache uncontrolled.

The repeatable harness is [benchmark_health_fast_response.py](../../performance/benchmark_health_fast_response.py); the [raw paired samples](2026-10-27-health-fast-response.json) include the exact commit and binary hash. The next everyday-task investigation should first isolate calendar parsing from its own fixed 100 ms HTTP fixture delays.
