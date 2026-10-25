# Independent v3 recommendation campaign

Status: the independent review approved the candidate identity, choice-safe product bundle, and campaign materials. The format-33 candidate is frozen and preregistered. All 18 blinded language choices are valid and complete; 18/18 chose Python across the three task classes. The matched build cohort is now in progress.

This campaign evaluates the current artifact-format-33 Veritas candidate. It supersedes neither the frozen format-30 independent-v2 campaign nor its partial session evidence. The v2 schedule and eight started implementation sessions remain preserved as a separate cohort and are not counted here.

## Bound candidate

- Veritas release: 0.1.0, artifact format 33, Linux ARM64 package.
- Archive: `../independent-v2/workspace/pinned/product/format33/veritas-0.1.0-linux-aarch64-format33.tar.gz`.
- Archive SHA-256: `46e22bb115f4ccce767c0e38e29db5cf3012d49f1556054f3f66214e47f30eb8`.
- Python: CPython 3.14.8; uv 0.13.4; Debian 12-compatible ARM64 image.
- Choice-safe product bundle: `product-information/`, digest recorded in `workspace-addendum.json`.
- Campaign images: Python `veritas-campaign-python:3.14.8`; Veritas `veritas-campaign-veritas:format33`. Their image IDs, Docker Engine ID, base image and dependency identity are bound in `campaign-lock.json`.
- The shared workspace and pinned v2 assets stay at `../independent-v2/workspace`; v3 uses versioned additions there and does not make a second workspace.

## Speed gate context

The latest one-host author calibration measured parity on the 15-trial scheduler/numeric/image suite and on a separate 16-pair 100,000-row JSON analysis. Veritas won the listed workload medians, including a narrow 4% margin on the data case. These results authorize proceeding to this recommendation evaluation; they do not prove general performance superiority. Packaged Python-bridge workflows and other hosts remain open.

## Frozen campaign identity

The independent reviewer verdict and reviewed file hashes are recorded in `independent-review.json`. `freeze-record.json`, `schedule.json`, and `prerun-records.jsonl` bind the reviewer result, task manifest, choice-safe bundle, format-33 package, image identities, campaign model, budgets, and randomized assignments. The 18 choices and 18 matched build pairs were assigned before sessions began. Do not reuse v2 assignments or count its eight partial format-30 sessions. The first choice had one duplicate interrupted retry; the valid session is recorded once and the retry is retained under `choice-evidence/excluded/`.

After the first two build pairs, inspection found two evaluator defects: the copied-delivery workspaces inherited `/tmp`'s `noexec` mount, and the Veritas source command bypassed `veritas.toml` by assuming a root `main.vr`. Original reports are preserved. The harness and Veritas launch configuration were corrected and hash changes are recorded in `protocol-corrections.md`; the original independent approval does not cover those corrected hashes. Any further scored runs must use new report filenames and disclose this amendment. Treat the first two pairs' original reports as diagnostic until they have been rerun under the corrected protocol.

## Run layout and hygiene

- Keep one canonical campaign workspace at `../independent-v2/workspace`; do not make another copy.
- Keep immutable format-30 inputs and evidence under `independent-v2/`. Keep format-33 candidate identity and choice-safe materials under this `independent-v3/` directory; put new v3 freeze, schedule, pre-run, session, and scoring records under this directory as well.
- Reuse the already provisioned Linux ARM64 Docker images and shared dependency archive. Run one candidate session at a time, then evaluate the completed batch sequentially. The format-33 runner uses the host `python3` only to stage/report artifacts; candidate Python, Veritas, and uv execute in the bound images.
- The format-30 host runtime directories recorded in the old workspace manifest are absent. Format-33 execution does not rely on them. Do not recreate them unless the runner inspection finds a concrete dependency.
- Hygiene removed 408 MB of ignored Rust `target/` output and Python `__pycache__` directories. The 1.1 GB compressed v2 evidence archive and 454 MB pinned shared workspace are retained because they are campaign evidence and inputs, not temporary build caches. The Docker images remain because this campaign is bound to them.
- The frozen v2 campaign and its partial evidence remain intact. This workspace is deliberately organized as versioned campaign records plus one shared pinned-workspace tree so each new cohort has a clear provenance without duplicating large inputs.

## Migration retention addendum (2026-10-25)

The original campaign notes above describe the workspace as it existed during the frozen run. During migration, the shared v2 pinned workspace was removed after preserving its compact v2 setup inputs. The bound format-33 Veritas candidate archive is no longer in this checkout; its recorded SHA-256 remains `46e22bb115f4ccce767c0e38e29db5cf3012d49f1556054f3f66214e47f30eb8`. The archive's exact byte size was not captured separately before workspace deletion. The retained source and lock/provenance files support a rebuild, but cannot establish the exact archive bytes.

Thirteen per-run `.delivery/` workspaces were removed (about 967 MB allocated). Fourteen compact run reports and their fourteen delivery-manifest files remain under `run-reports/`, along with the campaign prompts, choice evidence, hashes, source amendment, and package metadata. The 49,477,829-byte runtime-amendment package archive was also removed; its SHA-256 is `f6cba12f4ccfcf7c38b3fcd6167b7ac6d137a89f3003eb564fe4dfb733d56635`. These hashes document prior identities; they cannot restore the deleted packages or workspaces. Exact replay of the delivered candidates is no longer possible from this repository. The [migration record](../../docs/history/migration-2026-10-25.md) and [retention manifest](../../docs/history/migration-retention-manifest.json) detail the files retained and removed.
