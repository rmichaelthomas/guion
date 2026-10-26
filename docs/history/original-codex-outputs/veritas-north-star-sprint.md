# Veritas north-star sprint: one workback, two checkpoints

**North star:** Earn an assistant’s recommendation over Python for bounded, real developer tasks because a builder can start, understand, change, run, and deliver the program with less friction and a measurable practical advantage. Build today for future backends, platforms, and library ecosystems without tying Veritas to Rust, one host, or one application category.

**Sprint rule:** One continuous push. Engineering, calibration, packaging, and review preparation move together. Two checkpoints protect the final claim; neither is an RC or a request to stop.

## What prior language work changes about this build

Planes and Horizon make whole-program behavior the unit of proof: imported modules, repeated interactions, runtime inputs, and failure/resource behavior matter more than a successful isolated demo. Liminate contributes bounded syntax, explicit ambiguity handling, canonical output, and useful diagnostics. Decan contributes explicit context, exact support claims, and honest reporting when an adapter cannot preserve meaning. Veritas borrows these verification and boundary practices while remaining a general-purpose language; it does not inherit their specialized syntax or domain models. The detailed transfer is recorded in the repo's `docs/prior-language-build-lessons.md`.

## What the build has now proved

- The Rust-hosted compiler, VM, Cranelift native path, supervised Python bridge, package workflows, and macOS ARM64 / Debian-compatible Linux ARM64 distribution seams are working and covered by the existing tests.
- Veritas handles the canonical Richards result and runs the full N-body loop natively. Their published measurements remain mixed: Richards is slower than Python on the measured host; N-body is a workload-specific win. Neither is a universal speed result.
- The nine author-built candidates cover three task classes and 50 executable cases. Veritas and Python calibration references pass all 50 with matching outcomes. This is known-task calibration, not blind evidence.
- Format 30 adds generic streaming JSON column projection. The latest 100,000-row source-CLI comparison used 16 alternating fresh-process pairs and matched report bytes and stdout on every pair. Median was 0.126 s Veritas / 0.115 s Python. Against the earlier typed-column Veritas path at 0.218 s, that is about a 42% reduction; Veritas remains about 1.09× slower than Python on this task. The pure-Veritas control was 0.464 s vs 0.112 s. A later allocation change removed a second 5,100,867-byte input copy for this dataset. A matched 16-pair old/new binary comparison preserved outputs, but the timing difference was under 1%, so no further speed gain is claimed. See the [Python comparison](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-projection-optimized-timing.json) and [old/new allocation comparison](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-projection-borrow-comparison.json).
- The Rust workspace passes 196 tests; the source evaluation suite passes 85 tests with 7 expected installed-only skips. Both format-30 package targets pass 44/44 installed checks. The latest packages validate 10,960 Mac and 16,717 Linux entries. Archive SHA-256: macOS `6f02eba4a51e31aabca76bc256cbe29adead549a5677998a8786a92243d8ae9a`; Linux `080750d582c199e21e4098da7f21023958083c5602db725f2cb8226e9e20c203`. Strict Clippy passes across all workspace targets.
- Review-packet audit found that the Python calibration source and dependency pin were required by the instructions but absent from the packet hash inventory and shipped package. They are now explicit, hashed review inputs; the inventory covers 33 inputs, and the acceptance test checks their inclusion. A compact draft packet containing all 33 hashed sources plus one added file is available for independent review; it remains pending, not frozen.

All performance and task results above are author-run. They do not establish that an assistant recommends Veritas over Python.

Deliverables: [Mac ARM64 package archive](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-borrow-macos/veritas-0.1.0-darwin-arm64.tar.gz), [Linux ARM64 package archive](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-borrow-linux/veritas-0.1.0-linux-aarch64.tar.gz), [compact independent-review packet](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-independent-review-packet-draft.zip), and both [timing reports](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-projection-optimized-timing.json) and [allocation comparison](/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-north-star-json-projection-borrow-comparison.json).

## Workstreams to finish in the same sprint

### 1. Independent calibration and one cross-class pilot

An evaluator independent of the task authors and implementation reviews the nine briefs, all 50 expected outcomes, task overlap, language neutrality, and Python calibration references. Resolve mismatches before comparative results are collected. Then run an initial task and follow-up from each of the three classes in paired fresh Veritas/Python environments. Measure setup, first correct output, follow-up, interventions, active time, output quality, delivery, startup/package costs, and failures. Fix shared causes and rerun all 50 cases and both source-free package checks.

### 2. Freeze once

After review and pilot-driven fixes, pin the source revision, toolchains, dependencies, documentation, machine, time/tool budgets, permissions, prompts, and randomized pair order. Refresh the hash inventory and freeze once. Rebuild both supported packages from that frozen source and verify the shipped kit matches. Keep the current draft state until an independent reviewer signs off.

### 3. Run the full evidence block

Collect **18 matched pairs**: three classes × three briefs × two fresh pairs per brief, for 36 language runs. Separately collect **18 fresh assistant recommendation choices** before exposing any paired outcomes. Preserve failures and excluded attempts with reasons. Never count the author’s existing samples as unseen evidence.

### 4. Publish one scoped verdict

Apply the preregistered correctness, clean-delivery, practical-benefit, regression, and recommendation thresholds by task class. Recommend Veritas only where all guardrails support it. State where Python remains the better default and which measured engineering gap is still open.

## Checkpoints inside the sprint

**Checkpoint 1 — behavioral contract freeze.** Independent calibration is resolved; the three-class pilot and complete 50-case matrix pass; both packages match the frozen source. This is one evidence freeze, not a release candidate.

**Checkpoint 2 — recommendation verdict.** The 18 pairs and 18 independent choices are complete, analyzed under the frozen protocol, and reported with Python wins and uncertainty. If the independent reviewer or fresh sessions cannot be obtained, finish the engineering and publish the kit, but say the recommendation remains unproven.

## Current status

The engineering portion has advanced materially: a generic data path is implemented, edge cases are covered, and both format-30 packages pass installed acceptance. The north star is not met yet. Independent review, matched fresh-context task results, and direct recommendation choices remain. The next move is to complete those evidence dependencies as one campaign, not to relabel internal builds as RCs.
