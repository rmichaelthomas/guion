# SLA report — initial Veritas delivery

The initial request and scheduled `--team NAME` follow-up are implemented. The initial version remains preserved in `initial/initial.tar.gz`.

## Launch

On Debian 12-compatible Linux ARM64, from this directory:

```sh
./dist/sla-report/run --tickets /absolute/path/tickets.csv --policy /absolute/path/policy.json --as-of 2026-01-02T12:00:00Z --output /absolute/path/report.json
./dist/sla-report/run --help
```

The package includes Veritas 0.1.0 format33, uv 0.13.4, managed CPython 3.14.8, the Python standard library, and the declared adapter resource. No third-party Python dependencies. The package launch synchronizes its environment offline and does not need the original Veritas sources. Input/output paths can be outside the package. Nonzero status and actionable stderr diagnostics indicate failure. Validation completes before output replacement. `as_of` is normalized to UTC with `Z`; output is UTF-8 JSON. Ticket ordering uses team then ID with case-sensitive Unicode string ordering.

`delivery.json` declares the relocated argv and artifact set. Copy those paths preserving permissions and symlinks. The evaluator appends CLI arguments to the declared launch command. The source tree is excluded from relocated execution; the declared Python IO adapter is included as a normal package resource.

## Team filtering

Append `--team "Support North"` to select an exact, case-sensitive team name. The report array and summary cover only matching tickets; a missing team yields an empty array and zero counts. Omitting the option preserves the default report. All input rows are validated before filtering. See `FOLLOWUP.md` for the change, regression checks, and timing.

## Source setup and verification

In the frozen `veritas-campaign-veritas:format33` image, with the candidate mounted at `/candidate`, working directory `/candidate`, and networking disabled:

```sh
UV_OFFLINE=1 /opt/veritas/veritas sync --json .
/opt/veritas/veritas check .
/opt/veritas/veritas test .
python3 -m unittest discover -s tests
/opt/veritas/veritas build --json src/main.vr --output work/sla-report.vrb
```

Sync uses the existing pinned managed interpreter, zero third-party dependencies, and no wheelhouse packages. The shared wheelhouse remains available at `/opt/campaign-dependencies/wheelhouse` but was not needed. The source launcher is `./sla-report` (`VERITAS` may specify the tool executable).

## Packaging setup friction

The frozen image initially lacked both an interpreter download cache for package creation and `readelf`. Normal package creation was attempted and failed for each reason, with networking disabled. No network download or package install occurred.

1. `python3 work/offline_python.py` repacks **only** the image's existing `/opt/campaign-python/python/cpython-3.14.8-linux-aarch64-gnu` installation and writes uv custom-install metadata under `work/offline-python/`. It uses a local file URL and a recorded archive SHA-256. Repack took 1.621s, local archive 32,632,818 bytes. A local uv install validation took 1.94s. No Veritas-to-system-Python override was used.
2. With explicit coordinator authorization, already-installed GNU readelf (GNU Binutils for Debian) 2.40 was copied from the already-local `rust:1.97-bookworm` ARM64 image into `work/readelf/`, together with its libctf-nobfd, libsframe, libzstd and libz dependencies. `work/bin/readelf` sets a library path only for that tool. Nothing was installed. These are packaging-only tools and are excluded from delivery and relocated runtime mounts.
3. Successful normal packaging command:

```sh
PATH=/candidate/work/bin:/usr/local/bin:/usr/bin:/bin UV_PYTHON_DOWNLOADS_JSON_URL=file:///candidate/work/offline-python/downloads.json /opt/veritas/veritas package --json . --output dist/sla-report
```

Successful package creation spanned UTC 14:15:20–14:15:26 (about six seconds, coarse timestamp measurement). All setup friction is included in the session; see `work/SESSION_LOG.md` and `work/logs/` for failures and outputs. Commands must use the assigned candidate as their host working directory. Packaging requires the frozen image plus the already-local, coordinator-authorized build tooling.

## Relocation verification

A separate container of the frozen candidate image mounted only the completed package at `/application`, the independent test harness at `/checks`, and fresh test workspace at `/workspace`. It did not mount the original source tree or packaging tools. Commands:

```sh
/application/veritas verify-package --json /application
SLA_COMMAND='["/application/run"]' SLA_TEST_ROOT=/workspace python3 -m unittest discover -s /checks
```

Package verification passed, and all nine test methods passed (8.912s), including subcases for boundaries, status, ordering, malformed CSV/JSON, duplicate IDs, policy types, missing entries, timestamps, fractional precision, offsets, Unicode and output preservation. Source rerun passed nine methods (6.344s), plus one native Veritas threshold test. These are author-created tests only; no official acceptance results are claimed.

## Measurements and locks

Logical package file bytes (excluding symlinks): 125,233,019. Managed Python: 81,673,430 bytes; uv: 31,725,720 bytes; Veritas runtime: 10,294,800 bytes; compiled app: 9,900 bytes. First measured one-ticket package report after testing: 0.396s; second/warm report: 0.239s. These are single-run observations, not comparative performance claims. Initial offline source sync crossed 14:09:23–14:09:24 UTC (~1s, coarse).

- `veritas.lock`: `63792a3d9c5fed858e951dc294dda208581e84c80b75187b5e67b47893cf0198`
- `.veritas/python-bridge/uv.lock`: `bb392244428a2b87e16fcf51bfbeff00030b1eb46947fc4928aee5a4bf940074`

No known unresolved application failures in the author-created tests. Official acceptance remains unrun. Runtime support is the frozen Linux ARM64 target. No empirical correctness claim extends beyond the tests above. Initial version is preserved in `initial/initial.tar.gz` and its digest inventory before follow-up edits.
