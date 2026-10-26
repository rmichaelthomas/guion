# Veritas RC46 — installable current-host release

RC46 gives Veritas a real local installation path. `install.sh` places each release under a versioned directory, creates a `bin/veritas` command, preserves older release directories, and refuses to overwrite an unrelated command. It leaves shell startup files alone.

## Verified

- Formatting and strict Clippy passed.
- Rust workspace: 110 passed, 0 failed. Python acceptance: 20 passed, 0 failed.
- The Rust and Python suites initially hit host sandbox restrictions on loopback test servers and downloading the pinned CPython runtime. They passed when run with the host permissions those tests require.
- Installer checks passed for spaces and Unicode in the prefix, argument forwarding, refusing same-version replacement, and refusing a foreign command.
- The archive checksum passed. After extraction from a path containing spaces and Unicode, the installer installed the release to another such path. The installed CLI verified a source-free Python interop app; two offline launches returned identical output. The CLI ran a bundled Veritas example and printed `15` with the program's declared exit status `15`.

## What this does not establish

This is an unsigned macOS ARM64 release. It does not establish cross-platform installation or publisher identity. Dynamic library loads through runtime APIs and the offline Python wheel cache remain outside the native dependency scan. No independent head-to-head evidence supports recommending Veritas over Python yet.

The next product step already underway adds machine-readable results for assistants and editors. The broader recommendation gate and multi-platform packaging remain active parts of the build.

Archive SHA-256: `b8e256721b7fc15f74a9dedf4227a810b4fdd1725db31576b37aef1610e3069d`.

Files: [installable archive](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc46/veritas-0.1.0-darwin-arm64.tar.gz>), [machine-readable release evidence](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc46/packaging-rc46.json>), [installer checkpoint](</Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/outputs/veritas-foundation-rc46-checkpoint-v1-26.md>).
