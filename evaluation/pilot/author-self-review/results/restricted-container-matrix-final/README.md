# Restricted author calibration matrix

Status: author-run calibration only. These are existing implementations; they are not the fresh matched assistant pairs or recommendation choices required for the north-star claim.

The final matrix ran all nine tasks in initial and follow-up stages for both languages: **36 runner invocations, 100/100 executable cases passed**. Each invocation retained a JSON report with candidate tree hash, image identity, package and dependency digests, limits, elapsed case timings, and failures. The summary is `summary.json`.

Both candidate images run CPython 3.14.8 on Debian-compatible ARM64. The pinned base image digest is `sha256:44e000a07b48bef1517a2b14adc93c17af728ca98858b5e24c6e1b18de1b2ef2`; the Linux Veritas package digest is `080750d582c199e21e4098da7f21023958083c5602db725f2cb8226e9e20c203`; and the shared offline dependency bundle digest is `34a2a4d8d157839eea434267f38e475187031f081f358387859181e5be136803`. Image IDs are recorded in the summary.

The old macOS config-migrator pilot lock was not reused as valid Linux evidence. A copy was synced offline using the bundled managed CPython and shared wheelhouse; the exact project snapshot is in `candidates/config-migrator-veritas-linux/`, and its tree digest is recorded in the summary and each Veritas config-migrator report.

The initial diagnostic matrix under the previous container base remains at `../restricted-container-matrix/summary.json`; it includes the stale-lock failure and is retained to show how the platform mismatch was identified. It is not part of the final matrix.
