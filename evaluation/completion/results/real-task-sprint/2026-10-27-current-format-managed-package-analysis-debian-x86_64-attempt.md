# Debian x86_64 managed analysis package attempt — 2026-10-27

**Status: superseded by the successful Rosetta retry.** This records an environment failure during the first x86_64 attempt. It does not establish that the Guion package passed or failed its runtime acceptance; the completed result is in the [Debian x86_64 report](2026-10-27-current-format-managed-package-analysis-debian-x86_64.md).

The format-44 acceptance runner built the feature-enabled Guion CLI successfully, reused that build on retry, and began packaging the 100,000-row NumPy/orjson analysis. Packaging stopped during the native dependency audit, before relocation or application execution. The audit's `ldd` child aborted with exit code 134 while inspecting the bundled CPython 3.14.8 executable:

```text
Assertion failed: p_rcu_reader->depth != 0 (/qemu/include/qemu/rcu.h: rcu_read_unlock: 102)
ldd: exited with unknown exit code (134)
```

The run used an x86_64 Debian 12 container under Colima's ARM64 Linux VM. Docker had registered `qemu-x86_64` for foreign-architecture execution. QEMU's issue tracker documents the same RCU assertion among failures when running x86_64 binaries on ARM hosts; this points to the emulator path, not a Guion package finding ([QEMU issue 1147](https://gitlab.com/qemu-project/qemu/-/issues/1147)).

Because packaging stopped at dependency inspection, this attempt produced no accepted x86_64 bundle, relocation result, application output, package-size result, or meaningful launch timing. Debian x86_64 format-44 managed-package validation remained open until the subsequent Rosetta retry. The passing [Debian ARM64 report](2026-10-27-current-format-managed-package-analysis-debian-aarch64.md) is unaffected. The later attempt used Rosetta in the same Virtualization Framework VM and passed; see the completed report linked above.

The subsequent retry reused the corrected x86_64 uv resolver-and-wheel cache and the completed release build retained after this failure, avoiding another 34-minute QEMU compile.

Before retrying the full gate, the same VM was restarted with Rosetta enabled. A focused `ldd` check on the preloaded x86_64 CPython executable resolved all six system libraries and exited successfully. This checks the previously failing translation operation only; the full managed-package acceptance is still pending.
