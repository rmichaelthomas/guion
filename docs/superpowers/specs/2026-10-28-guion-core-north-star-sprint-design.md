# Guion core north-star sprint design

## Purpose

Make Guion a stronger default choice for an unfamiliar agent doing useful work, with a particular focus on a fast, capable, understandable Python-free core. The north star is a general-purpose language that can outperform Python and become the practical choice for agents. Optimize toward that ambition without treating a narrow benchmark or a guessed percentage as proof it has been reached.

This is a design for a future implementation sprint, not authorization to start one now. The spec starts from commit `81a5450` on the PR #21 branch. Before implementation, inspect the then-current default branch and PR #21, incorporate relevant merged changes, and choose and record the actual base. Do not silently overwrite other work. The spec lives in an isolated worktree; create or reuse an appropriate isolated worktree for implementation.

## Outcomes and priorities

1. Establish a reproducible Python-free baseline for useful work and locate the most consequential measured core costs.
2. Improve the compiler, runtime, allocation, or text path where evidence points, and retain improvements supported by correctness and comparative measurements.
3. Make the HTTP response and text/JSON path coherent, typed, discoverable, and recoverable.
4. Exercise and improve capability declarations, enforcement, diagnostics, and resource behavior for realistic agent workflows.
5. Make tooling, compatibility information, examples, and supported-target claims consistent and actionable.
6. Run an unfamiliar-builder challenge and repair the highest-impact core and documentation obstacles it reveals.
7. Re-run the measurements, publish an understandable before/after report, and update the README and agent-facing documentation.

Treat these as one sprint with checkpoints, not a promise to achieve universal language maturity or guaranteed speed superiority. Finish the named outcomes and disclose any unmet acceptance criteria with evidence and a concrete next action. Preserve user-visible semantics unless a documented migration is explicitly designed and tested. Python interoperability remains optional; do not spend this sprint on bridge micro-optimization unless a core outcome demonstrably depends on it.

## Shared execution environment and hygiene

- Prepare one reusable, pinned container image and one persistent container for the Linux portion. Inventory the repository's actual toolchains, test fixtures, Python baseline dependencies, and optional features first. Install or prefetch all known dependencies once, record versions and image identity, and verify an offline build before long runs. A missing dependency is fixed in the same environment and documented; do not repeatedly recreate environments.
- Use the same container, mounted isolated worktree, and bounded, dedicated build/cache paths for sequential tests and experiments. Run offline after preparation, using local HTTP fixtures and loopback where needed; disabling external network must not disable the local fixture traffic under test. Network-dependent checks that truly cannot be reproduced offline are identified and handled explicitly rather than misreported as passed.
- A Linux container is not a macOS environment or proof of other architectures. Measure native macOS performance on the host where needed, and make target-specific claims only for the environment actually exercised. Do not create additional VMs or containers for routine checks. If a required target genuinely needs another environment, use one at a time and document why.
- Check available disk, container/VM state, cache size, and active workload before expensive work; set practical CPU and memory limits compatible with meaningful measurements. Run heavy work sequentially. Keep temporary files and durable results under the repository or its dedicated worktree. Do not use a shared cache for competing builds unless its isolation and integrity are clear.
- Record starting and ending disk use. Remove only scratch, containers, images, and caches created for this sprint when no longer needed; preserve source, test fixtures, raw measurements, and useful reports. Never prune shared Docker/Colima resources or run a cleanup that could remove another build or the worktree. Stop processes started by the sprint when finished. Commit at sensible completed checkpoints, rather than after every test.
- Request the access actually needed for the environment once at the outset, within the execution platform's permission rules. Prior user authorization supports the work but does not override technical access controls. If an access or automatic approval block remains, identify the precise action and reason, continue independent work, and report it; do not repeatedly retry the same denied action or claim it passed. Do not assume all network, host profiling, or external evaluator access is available.

## Measurement and optimization

Freeze representative equivalent Guion and Python tasks before optimizing: fresh source startup and short scripts; text and JSON processing including a larger input; collections/control flow; HTTP response handling against the same local fixture; and at least one real application workflow. Keep Richards as a useful diagnostic alongside these tasks. Include relevant artifact comparisons and a separately labeled optional-bridge lane only when informative. Each timed run must check equivalent output or an explicit correctness oracle.

Record hardware, OS, versions, build flags, commit, dependency versions, workload sizes, sample counts, execution order, warmup and cache conditions. Use randomized paired repeated runs and report medians, uncertainty intervals, and raw samples; distinguish warm OS caches from genuinely cold storage. Measure peak memory with a working method and state whether it includes child processes. If a sampler fails, investigate alternatives and record precisely what remains unmeasured. Measure native-host performance separately from container results. Avoid background builds and competing benchmark runs while timing.

Profile before selecting each optimization. Set the success comparison and correctness checks before running the candidate. Compare against an immediately relevant baseline; repeat a promising marginal result when noise makes the conclusion uncertain. Keep demonstrable improvements without correctness, security, or meaningful memory regressions; revert inconclusive or regressing candidates and retain their evidence. Do not conflate batching work in Python with making the Guion runtime itself faster. Report both absolute times and ratios and avoid extrapolating from one workload to all programs.

## HTTP and text design

Choose a canonical typed HTTP response path with a clear status, headers, and body. Add explicit UTF-8 decoding of bytes with a typed, recoverable invalid-encoding error and a straightforward way to parse a text response as JSON. Respect existing size and policy limits and preserve arbitrary binary bodies. Decide API names and migration details after inspecting existing compiler and runtime conventions; document the decision in the implementation plan. Existing `http_request` behavior must remain compatible or have an explicit, tested migration path; do not silently change its result type.

Demonstrate text/JSON success, invalid UTF-8, malformed JSON, non-2xx response values, host denial, timeout, and response-size limits. Cover source and artifact execution where applicable, and update canonical examples and agent reference together. Measure performance and request counts on the representative workflow; do not claim fewer network calls or faster execution merely from an API cleanup.

## Safety, tooling, and compatibility

Audit declared capabilities against actual runner enforcement and inspection output, including dynamic URL hosts, redirects, file roots, environment reads, cancellation, and resource ceilings. Verify missing grants produce actionable failures and guest code cannot expand host grants. Fix confirmed defects with regression coverage. Explain the threat boundary accurately: runtime capability checks are not operating-system process isolation, and static declarations cannot fully infer arbitrary dynamic behavior.

Check source/artifact compatibility and upgrade diagnostics for recently introduced syntax and APIs. Document supported targets from current evidence, not assumptions; target checks that require a different platform remain explicitly scoped. Validate structured tool output and actionable diagnostics for agents. Treat single-file atomic replacement and multi-file transactions as distinct guarantees. Add multi-file transaction machinery only if a chosen sprint workflow requires it and a concrete crash/recovery contract can be tested. Python bridge failures are addressed if they block a selected outcome; they are not required for Python-free core completion.

## Unfamiliar-builder challenge

Freeze a useful application brief and acceptance cases before the evaluation. Give a builder who has not participated in the implementation the normal Guion documentation and tools, with a maximum two-day window. An isolated agent evaluator is acceptable and must be identified as such; it is not evidence of broad human or agent adoption. Capture time, attempts, errors, documentation gaps, completed functionality, and reproducible acceptance results. Avoid supplying hidden solutions or changing the rules mid-run. Respect the actual access and data-sharing permissions for the selected evaluator; previously approved specific studies do not automatically authorize a different transfer. Fix the highest-impact obstacles in scope and rerun the affected acceptance checks. If no suitable evaluator can be run, say exactly why and leave this outcome open rather than substituting the implementer's self-assessment.

## Sprint acceptance and handoff

The sprint is complete when:

- The environment and offline dependency inventory are reproducible; relevant correctness and regression checks pass, with failures investigated rather than silently skipped.
- Baseline and final data, raw samples, correctness oracles, memory method, and comparison limitations are retained in the repo. Each retained speed change has evidence tied to its commit; unsuccessful candidates are clearly labeled.
- The typed HTTP/text path, failure behavior, compatibility decision, and examples satisfy the cases above.
- The capability and tooling audit identifies exercised boundaries, fixes verified defects, and records residual limitations and target-specific evidence.
- The builder challenge is performed and its findings addressed as specified, or its specific unresolved blocker and resulting incomplete status are reported.
- README, agent reference, migration guidance, and a plain-English results report reflect the final code and measured claims. The report identifies what got faster, what did not, remaining gaps, and the next recommended target without relying on an arbitrary percentage.
- Work is committed in logical checkpoints; the final handoff names the branch, commits, verification results, remaining issues, and cleanup performed. Create or update a PR only if authorized during implementation. Do not merge without the user's direction.
