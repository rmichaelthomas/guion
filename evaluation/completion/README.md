# Veritas product completion evidence

The final Veritas evidence and the later Guion rename acceptance results have distinct product identities. Use the [evidence index](../../docs/history/evidence-index.md) for their exact report paths and scope.

This directory records completion work for the Veritas build plan. It is a new evidence stream for Tasks 1–11, separate from the frozen independent-v1, independent-v2 and independent-v3 campaign records. The frozen result remains 18 Python choices and 0 Veritas choices; those records are not revised or re-scored here.

The nine existing application briefs are known regression applications. Each brief registers both its initial behavior and its follow-up change. They measure product correctness and maintainability during implementation; they are not presented as unfamiliar builder evidence.

| Application | Brief |
|---|---|
| SLA report | `evaluation/briefs/independent/01-support-sla-report.md` |
| JSONL summary | `evaluation/briefs/independent/02-jsonl-retention.md` |
| Release notes | `evaluation/briefs/independent/03-release-notes.md` |
| Calendar conflicts | `evaluation/briefs/independent/04-calendar-conflict-report.md` |
| Webhook replay | `evaluation/briefs/independent/05-webhook-replay.md` |
| Health digest | `evaluation/briefs/independent/06-service-health-digest.md` |
| Config migrator | `evaluation/briefs/independent/07-config-migrator.md` |
| Documentation index | `evaluation/briefs/independent/08-documentation-link-index.md` |
| Lockfile auditor | `evaluation/briefs/independent/09-lockfile-auditor.md` |

## Required completion record

`status.json` is the live task ledger for this build. It records the active task, next command, evidence paths, required reports, required targets and unsatisfied goal IDs. `results/` contains structured reports written by `cargo xtask`.

The full closeout requires all required reports and checks to be present and passed, both supported targets where required, and no skipped required case. A missing report, missing test/check, incomplete report or skipped target remains incomplete. `cargo xtask closeout --check-schema` checks report shape only and never claims product completion.

Run the defined sequence in order: conformance inventory and compiler coverage, three-stage bootstrap, data and service libraries, runtime, dependency/product lifecycle, applications, releases, performance, fresh builders, and closeout. Checkpoints report evidence and remaining failures while implementation continues under the same plan.

## Target and measurement scope

Release targets are macOS ARM64 and Debian-compatible Linux ARM64. The completion evidence must report the exact installed product, target identity, source revision and working-tree digest, compiler seed, standard-library and runtime identities, artifact identities, commands, checks, and raw performance samples. Python baselines remain comparison tools; core Veritas builds and delivered applications must not depend on Python or `uv`.

Performance claims are limited to workloads with correct outputs and reproducible matched measurements from the final installed build. The report retains ties, regressions, raw samples and uncertainty intervals. Python comparisons of Unicode behavior record `unicodedata.unidata_version`; deltas from ICU4X's recorded Unicode 18.0.0 dataset are reported separately from algorithm mismatches. Fresh builder evidence is a separate three-brief matched evaluation and does not alter the frozen campaign scores.
