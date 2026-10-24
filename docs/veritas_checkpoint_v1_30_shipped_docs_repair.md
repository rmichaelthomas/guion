# Veritas checkpoint v1.30 — repair from shipped docs and reports

## Change

The evaluation kit now includes an installed-product acceptance test for the basic assistant repair loop. It creates a small program with an unknown-name error, asks the installed CLI for a structured check report, consults the installed reference and host API docs for the output function, fixes the typo, then checks, runs and tests the program. The test runs outside the source checkout and requires an explicit installed-release root and CLI path.

This verifies that the shipped documentation and structured diagnostics support a small repair from failure to working program. The scripted case is authored by the project and is a known acceptance workload; it is not independent evidence about model preference or performance.

## Evidence

- Against the installed RC49 release, the test read only the installed docs and invoked the installed CLI from a temporary project outside the source checkout.
- The broken source returned `E_NAME_UNKNOWN` at the expected line; after repairing `prnit` to the documented `print`, check succeeded, run returned `repaired`, and one source test passed.
- RC50 will rerun this test from the packaged evaluation kit and run the wider Python acceptance suite against the installed binary.

## Limits and next work

The scripted test demonstrates one simple typo repair. It does not simulate an independent model's reasoning, diverse repair categories, or follow-up requirement changes. Broader app lifecycle, package and comparative recommendation evidence remains active.
