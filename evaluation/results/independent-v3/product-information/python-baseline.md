# Python campaign profile

This campaign evaluates CPython 3.14.8 on Linux ARM64. The available launcher is `python3`; no `python` command is assumed. Candidates may use the standard library and the exact third-party distributions listed in the campaign dependency lock. Those dependencies are available only through the pinned offline wheelhouse, managed by uv 0.13.4. Network downloads and undeclared system interpreters are outside the campaign environment.

The Python candidate is a project with a lockfile and a runnable entry point. The evaluator first runs it from the candidate workspace, then copies only the declared delivery artifacts into a clean directory and runs it there. The task brief, fixture inputs, network permissions, container resource limits, active-time budget, and tool-call budget are the same as for Veritas.

This profile describes the pinned campaign baseline, not every Python implementation or deployment option. The broader language reference is [Python 3.14 documentation](https://docs.python.org/3.14/).
