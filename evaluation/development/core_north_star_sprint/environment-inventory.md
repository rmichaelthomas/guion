# Guion core north-star sprint environment inventory

Recorded before benchmark preparation on 2026-10-28.

## Source

- Branch: `codex/guion-core-sprint-spec`
- Sprint implementation baseline: `a1c0d40f1445e6806238ae252fc1842d0a18a6c1` (current main merged with the sprint design)
- Main commit incorporated: `2058967ffd8f334c1edaff59679abbe1c1fd4890`
- Worktree: `.worktrees/guion-core-sprint-spec`

## Host

- macOS Darwin 27.0.0, arm64 (Apple Silicon T6000)
- Rust/Cargo 1.97.1; host CPython 3.14.7
- Host data volume: 926 GiB total, 820 GiB used, 66 GiB available (93% used)

## Linux runtime

- Existing Colima `default` profile: Running, aarch64, 4 CPUs, 8 GiB RAM, 60 GiB disk, Docker runtime. It was queried read-only; this task did not start or restart Colima.
- Docker client 29.8.1; server 29.5.2; Linux Ubuntu 24.04.4 LTS, aarch64.
- Pinned existing image: `guion-feed-digest-debian-aarch64@sha256:6a45e30e542ab1dc5600a1e5a3a7579e3d520107a5fb0c1471af6e6f1e3ca1ac`; Debian 12 (bookworm); image size 540,271,421 bytes.
- One sprint container: `guion-core-north-star-sprint`, ID `4e0bf318731255611f9978939cf25d1a5dfe49a6e72a2da893fb330a82f0054b`, limited to 4 CPUs and 6 GiB memory. The isolated worktree and its Git metadata are mounted; Cargo home and target are dedicated to this container.
- Container toolchain: Rust/Cargo 1.97.1; system CPython 3.11.2; pinned CPython 3.14.7 was built from the official Python.org source with PGO (`./configure --enable-optimizations --with-ensurepip=no`, `make -j3`, `make altinstall`) at `/opt/cpython-3.14.7/bin/python3.14`. Source tarball SHA-256: `62859805f6fdf25e2bcbf3fa3217801e1996887ca33e6a2af80674bdfa2dbe07`.
- Container root filesystem at inventory: 59 GiB total, 27 GiB used, 29 GiB available.
- Shared Docker inventory at inventory: 34 images (27.83 GB), 7 volumes (806.4 MB), 112 build-cache entries (6.748 GB). These are not sprint-owned and will not be pruned. The only pre-existing container is the unrelated stopped `conv-cap`; it is untouched.

## Frozen baseline evidence (2026-10-28)

- Locked dependency preparation completed with `cargo fetch --locked`; `cargo check --locked --offline --workspace` passed. A release build of `guion-cli` also passed offline. Build, check, fetch, Python configure/build, and benchmark logs are retained beside this inventory.
- The Linux source-loop baseline passed on revision `a1c0d40f1445e6806238ae252fc1842d0a18a6c1`; its 7-trial lanes include 2 warmups, and the correctness-checked suite records 16 paired data trials plus raw samples. Report: `evaluation/completion/results/real-task-sprint/2026-10-28-linux-arm64-baseline.json` (SHA-256 `9b01eeb898600b6f715de13e81a97c56e0fec23566e5d53d43f5132e47f1f6b3`); measured executable SHA-256 `d3ed076fdc32a7a53f0f923675f78cb8c80b5f80e98759dd0866f7df98f9ff34`.
- Timed durations cover each child process from startup through exit; output-oracle checks run immediately after each sample and are excluded from the timer. Fixture-server startup is also excluded.
- The short-source and loopback HTTP baseline passed with 15 randomized paired trials and 2 warmups per arm; all timed outputs matched the fixed oracle and the fixture observed exactly 34 requests (17 per arm). The JSON body was 65,579 bytes with SHA-256 `46e2a169feef79fd603ef170f728913cbcc82b6aa424e6594624cb2f5d912857`. Report: `evaluation/completion/results/real-task-sprint/2026-10-28-short-http-baseline.json` (SHA-256 `0b7969c470e938141f1a97182cd4f7bdb3a033cbaea884dadcda74acea6fc855`); measured executable SHA-256 `f7d20351b2bfad13fb8e515cfadfff46ed964792e7d0528ffd1b95aa05c28290`.
- The Linux source-loop medians show a measured JSONL gap: 20,000 rows took 0.555 s in Guion source, 0.554 s as an artifact, and 0.121 s in the Python data reference; 200,000 rows took 5.326 s in Guion source, 5.323 s as an artifact, and 0.799 s in Python. These figures apply only to this generated JSONL workflow and the specified Linux ARM64 container.
- A separate untimed runtime profile checked both JSONL outputs against the Python oracle. At 200,000 rows it reported 31,495,975 successful guest allocation-budget charge events (about 3.92 GB estimated gross charges), 4,173,319 guest conditional branches, 400,947 record charges, and no list copy-on-write activity. The counters are estimates, not host allocator calls. Profile: `evaluation/development/core_north_star_sprint/jsonl-runtime-profile.json` (SHA-256 `b9b642731e35996365bf67dc7ccf914def214896346ce2117e656da8315ae85e`). The container has neither `perf` nor `strace`; `perf_event_paranoid` is 4, so per-function CPU sampling was unavailable. The profile identifies a high-allocation JSONL execution path but does not attribute CPU time to individual runtime functions.
- The container was disconnected from Docker's external bridge after dependency preparation. Follow-on Cargo work is offline; the HTTP fixture remains reachable on loopback. No shared image, cache, volume, or unrelated container was pruned or modified. At the end of baseline work the container had 27 GiB available; host storage usage must be recorded again at sprint end.
- Python evaluation relies on the standard library (`json`, `sqlite3`, `urllib`, and related modules); no third-party Python packages were needed. CPython 3.14.7 is pinned in `workload-manifest.json`; repository application fixtures, famous benchmarks, and independent JSONL oracle are listed there. Cargo CLI/runtime defaults are empty feature sets; the optional `python-interop` feature is excluded from this Python-free core suite. Cranelift dependencies are locked at 0.135.0.

## Work policy

- All heavy build and measurement work is sequential and offline after dependency preparation. HTTP benchmarks use loopback fixtures.
- Temporary build and dependency data stays under `/var/cache/guion-core-north-star` in the sprint container. Durable reports and raw data stay under the sprint evaluation directories.
- No project-level `AGENTS.md` was found in the Guion repository; none was added or changed.
