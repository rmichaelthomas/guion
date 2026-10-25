# Timing and artifact evidence

Session start is the creation time of the opened contract: `2026-10-25T09:58:59.094268Z`. Runtime was Python 3.14.7, standard library only.

| Phase | Duration | Measurement boundary |
|---|---:|---|
| Setup | 51.495298 s | Contract creation to start of initial implementation (`09:59:50.589566Z`) |
| Build | 93.668499 s | Initial implementation start to passing initial behavior checks (`10:01:24.258065Z`) |
| Follow-up | 73.560170 s | Later due-soon source edit start (`10:02:07.713075Z`) to old-behavior and due-soon checks passing (`10:03:21.273245Z`) |
| Installed launch | 0.066778 s | Median of five fresh `./bookloom list available` processes from the relocated path |

The five launch samples, full CLI command logs, and outputs are in [relocation_checks.json](relocation_checks.json). Final 22-command verification is in [final_verification.json](final_verification.json). Phase check records are [stage1.md](stage1.md) and [stage2_and_relocation.md](stage2_and_relocation.md).

The final user-facing artifacts total `12,263 bytes`: `10,671` bytes for `bookloom.py`, `127` bytes for the launcher, `69` bytes for `bookloom.toml`, and `1,396` bytes for `OPERATOR.md`. Executable source footprint is `10,798 bytes`.

The initial validation harness had one assertion failure because its expected available-list membership was inverted. The assertion was corrected, the scratch database was reset, and all 15 initial checks passed on rerun. A final evidence path predicate also needed resolution of its relative `..` segment; the corrected predicate and full verification passed. No application operation failed unexpectedly.
