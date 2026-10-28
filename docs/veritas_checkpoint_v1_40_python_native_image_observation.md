# Veritas checkpoint v1.40 — Python native image observations

Status: implemented; focused runtime and CLI integration checks pass on macOS ARM64.

Guion `run --json` includes `python_native_image_observation` and `python_native_images`. The Python sidecar takes a baseline dyld snapshot at startup, then takes another snapshot when a Python import, `ctypes.dlopen`, or changed dyld image count indicates a possible native-image change. Cached calls with no observed change skip the full enumeration. It reports paths newly resident at those observed boundaries, including images loaded by module import or APIs such as `ctypes.CDLL`. The runtime retains and deduplicates observations over the instance lifetime, including when the Python call raises. Hosts without the macOS dyld API report `unsupported`; a run with no Python bridge call reports `not_used`.

A macOS runtime integration test compiles a small dylib, loads it through Python `ctypes`, and checks the returned value and reported canonical path. A CLI integration test repeats that flow through `guion run --json` and verifies the structured report and program exit code.

## Limits

This is an event-triggered boundary snapshot, not a complete loader trace. A library loaded and unloaded inside one Python call can be absent from the post-call snapshot; a balanced image replacement that keeps the image count unchanged and emits neither observed Python event may also be missed. The observation is diagnostic runtime data, not a sandbox, security control, license assessment, or proof of all files accessed. `NATIVE-DEPENDENCIES.json` remains the static package inventory; offline-cache wheel images are still not inventoried there. macOS is the only host with an implemented observer today.

## Next work

- Attribute Mach-O images inside offline wheel-cache payloads and connect their hashes and ownership to package metadata.
- Run the full workspace and installed acceptance suites, then build and relocate the next release candidate.
- Continue cross-host validation and the assistant recommendation evaluation.
