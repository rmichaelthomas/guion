# Veritas checkpoint v1.41 — offline wheel-cache native provenance

Status: implemented and covered by the compiled-wheel package integration test.

The `NATIVE-DEPENDENCIES.json` report now has separate `images` and `cached_images` lists. `images` remains the set of native files in the runnable app, managed interpreter, installed Python environments and declared resources, with load edges resolved or rejected when unresolved. `cached_images` records Mach-O files inside extracted distributions in the offline uv cache without claiming that those copies are loaded by the current app. Each cache record contains the package-relative path, SHA-256, byte size, architectures, raw Mach-O load names, and Python distribution name/version with the owning cached `RECORD` path.

Package verification checks cached image and RECORD paths, validates their recorded hashes and ownership labels, and continues to verify the immutable offline-cache payload digest. This makes accidental changes visible through both the package integrity boundary and the per-image inventory. The evidence is not authenticated because package metadata is unsigned.

The package integration test confirms NumPy native extensions are present in `cached_images`, attributed to NumPy's cached RECORD, and remain valid after app relocation. The full workspace passes 122 Rust tests, including the compiled-wheel relocation tests. Formatting, strict Clippy, archive validation, installation, and all 22 shipped acceptance checks pass on macOS ARM64.

## Scope

The scanner records load names for cache copies but does not resolve cache load edges against their eventual installed paths. `NATIVE-DEPENDENCIES.json` remains a static package inventory. `run --json` reports runtime dyld boundary observations from Python calls; images loaded and unloaded inside a call may not appear. The package is unsigned, and this inventory is not a legal-compliance assessment.

## Remaining work

- Validate native package creation and runtime behavior on additional host targets.
- Add a supported dependency update and recovery path to the installed product.
- Complete the independent recommendation evaluation against representative developer tasks.
