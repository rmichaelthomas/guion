# Veritas checkpoint v1.25 — native Python binary ownership

## Change

`NATIVE-DEPENDENCIES.json` now links native files inside Python `site-packages` to their installed distribution by reading that distribution's wheel `RECORD`. Each owner record includes its name, version, environment, `RECORD` path, license status and the exact license-file records copied into the package. Native extensions in `site-packages` without an owner, and files attributed to multiple distributions, fail packaging. Other scanned images receive an explicit runtime/toolchain classification or are listed in `unattributed_images`.

`verify-package` confirms each native Python owner exists in `PYTHON-DEPENDENCIES.json`, that its license records match, and that the referenced `RECORD` and license files are present. Existing package hashes still detect report and binary changes.

## Evidence

- `cargo fmt --all --check` and strict workspace Clippy passed.
- `cargo test --workspace --offline`: 110 passed, 0 failed. The Python acceptance suite: 20 passed, 0 failed, using the pinned uv, managed CPython and offline cache.
- The compiled-wheel integration package includes charset-normalizer, NumPy and orjson native extensions. Every native file under `site-packages` has one owner; each owner's license metadata exactly matches `PYTHON-DEPENDENCIES.json`; the report lists no unattributed images.
- The app verified after relocation and ran twice offline. Tampered license, native image and Python runtime contents were rejected. A forged native owner was also rejected after deliberately refreshing the unsigned native-report checksum; the package verifier cross-checked it against the separate Python distribution inventory.
- Release assembly now packages the source-free NumPy/orjson interop example with the shipped Veritas binary and verifies it before archiving.

## Limits

Wheel `RECORD` identifies which distribution installed a file; it does not prove the source or copyright ownership of vendored code within that distribution. Native libraries loaded only through runtime APIs and files in the offline wheel cache remain outside the current scan. Package hashes are unsigned. This milestone does not prove general-purpose readiness or assistant recommendation superiority.
