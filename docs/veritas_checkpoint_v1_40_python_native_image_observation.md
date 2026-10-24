# Veritas checkpoint v1.40 — Python native image observations

Status: implemented; focused runtime and CLI integration checks pass on macOS ARM64.

Veritas `run --json` now includes `python_native_image_observation` and `python_native_images`. The Python sidecar takes a dyld image snapshot at startup and after each bridge call. It reports paths newly resident at those boundaries, including images loaded by module import or by Python APIs such as `ctypes.CDLL`. The runtime retains and deduplicates observations over the instance lifetime, including when the Python call raises. Hosts without the macOS dyld API report `unsupported`; a run with no Python bridge call reports `not_used`.

A macOS runtime integration test compiles a small dylib, loads it through Python `ctypes`, and checks the returned value and reported canonical path. A CLI integration test repeats that flow through `veritas run --json` and verifies the structured report and program exit code.

## Limits

This is a boundary snapshot, not a complete loader trace. A library loaded and unloaded inside one Python call can be absent from the post-call snapshot. The observation is diagnostic runtime data, not a sandbox, security control, license assessment, or proof of all files accessed. `NATIVE-DEPENDENCIES.json` remains the static package inventory; offline-cache wheel images are still not inventoried there. macOS is the only host with an implemented observer today.

## Next work

- Attribute Mach-O images inside offline wheel-cache payloads and connect their hashes and ownership to package metadata.
- Run the full workspace and installed acceptance suites, then build and relocate the next release candidate.
- Continue cross-host validation and the assistant recommendation evaluation.
