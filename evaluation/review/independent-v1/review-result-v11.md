# Independent review result — Veritas packet v11

**Verdict:** Revise. **Reviewer:** Codex, independent technical reviewer. The reviewer reported no role in authoring the briefs, fixtures, acceptance rules, Veritas implementation, or packet. This was a read-only static review; the reviewer did not run tests, builds, image builds, installations, candidate tasks, schedules, pre-runs, or formal sessions, and did not inspect comparative outcomes.

## Review findings

All nine task briefs and their oracles were approved. The reviewer approved the 114-case manifest and confirmed the new schema-2 absent/Boolean parallelism cases, exact expiry-day case, and explicit offline download restrictions. The protocol was approved with its combined no-build choice and matched implementation gate; disagreement withholds a class recommendation. The 20% practical-benefit rule and sample limitations were judged clear.

The only required revision was that the choice-safe product bundle still exposed prior development workloads and evaluation details:

- `product-information/README.md` included packaging commands for a development project and described known workloads, independent Python references, parity/relocation acceptance, and implementation milestones.
- `product-information/reference.md` described a known feed-digest workload and its Python-baseline acceptance harness.

The reviewer required removing those commands and passages, replacing them with neutral product-capability wording, and recomputing the product bundle digest and outer artifact hashes. The v11 packet’s ZIP, all 59 artifact hashes, campaign lock, v22 archive, dependency digest, product bundle digest, calibration metadata, and packaged protocol alias were otherwise confirmed consistent.

This report is included as a remediation checklist for v12. Its revise verdict is not approval of the next packet.
