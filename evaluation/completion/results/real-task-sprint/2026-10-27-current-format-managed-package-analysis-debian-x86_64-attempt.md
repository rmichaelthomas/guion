# Debian x86_64 managed analysis package attempt — 2026-10-27

**Status: incomplete; no package result.** This records an environment failure during the first x86_64 attempt. It does not establish that the Guion package passed or failed its runtime acceptance.

The format-44 acceptance runner built the feature-enabled Guion CLI successfully, reused that build on retry, and began packaging the 100,000-row NumPy/orjson analysis. Packaging stopped during the native dependency audit, before relocation or application execution. The audit's `ldd` child aborted with exit code 134 while inspecting the bundled CPython 3.14.8 executable:

```text
Assertion failed: p_rcu_reader->depth != 0 (/qemu/include/qemu/rcu.h: rcu_read_unlock: 102)
ldd: exited with unknown exit code (134)
```

The run used an x86_64 Debian 12 container under Colima's ARM64 Linux VM. Docker had registered `qemu-x86_64` for foreign-architecture execution. QEMU's issue tracker documents the same RCU assertion among failures when running x86_64 binaries on ARM hosts; this points to the emulator path, not a Guion package finding ([QEMU issue 1147](https://gitlab.com/qemu-project/qemu/-/issues/1147)).

Because packaging stopped at dependency inspection, this attempt produced no accepted x86_64 bundle, relocation result, application output, package-size result, or meaningful launch timing. Debian x86_64 format-44 managed-package validation remains open. The passing [Debian ARM64 report](2026-10-27-current-format-managed-package-analysis-debian-aarch64.md) is unaffected. The next x86_64 run must use a working translation path such as Rosetta in the same Virtualization Framework VM, or a native x86_64 Linux host.

The corrected x86_64 uv resolver-and-wheel cache is retained only in ignored repository scratch until the next acceptance attempt. The stopped acceptance container retains the completed release build so the retry does not need another 34-minute QEMU compile. The VM had no running containers after the attempt.

Before retrying the full gate, the same VM was restarted with Rosetta enabled. A focused `ldd` check on the preloaded x86_64 CPython executable resolved all six system libraries and exited successfully. This checks the previously failing translation operation only; the full managed-package acceptance is still pending.
