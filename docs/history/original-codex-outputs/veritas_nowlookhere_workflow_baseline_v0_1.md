# Veritas / NowLookHere workflow baseline v0.1

**Date:** 2026-10-22  
**Purpose:** Measure a real command-line build-to-phone loop as a candidate Veritas workflow test. This is a local observation, not a benchmark of Veritas execution speed.

## What was measured

I read the existing `/Users/rmichaelthomas/strands/scripts/install-device.sh` and ran it twice unchanged, then ran a temporary instrumented copy once to separate phases. Each run built the Release iOS app, installed it on the paired iPhone, and launched it. No NowLookHere source or script was edited. The repository already had unrelated working-tree changes before the run; the same changes remained afterward.

The first observed run took **29.52 seconds**. A repeat with the build cache warmed took **19.16 seconds**. The instrumented repeat took **21.88 seconds**; timing markers added a small amount of overhead.

The instrumented run measured:

| Step | Time |
|---|---:|
| Find paired device | 0.17 s |
| Generate Xcode project | 0.09 s |
| First Xcode build attempt | 11.54 s |
| Fallback Xcode build | 4.63 s |
| Install on phone | 4.65 s |
| Launch app | 0.50 s |

The first build attempt failed because Push Notifications and Associated Domains are not enabled for this app ID. The installer then rebuilt without those entitlements and successfully installed and launched the app. That fallback behavior is already in the script. The first observed run is not confirmed to be a cold build: the state of Xcode's DerivedData before it was not recorded. These three timings are useful as a starting point, not as a stable performance claim.

## What this says about Veritas

NowLookHere is a good **workflow-level** case: it is real work, has a clear finish line (the app opens on the phone), and exposes the difference between “the code compiled” and “the builder can actually try it.” But it is not a clean test of language runtime speed. Xcode controls the Swift compilation, and the measured wait is mostly Xcode plus installing the app.

The existing script already invokes `python3` for its small device-list JSON lookup. The `python`/`python3` problem Rob described is with commands suggested by assistants elsewhere; it remains a useful assistant-usability check, but it is not a failure in this script.

There may be an opportunity to avoid the 11.54-second failed build attempt by detecting the missing app capabilities before the first build. That would be an improvement to this particular workflow, not evidence that Veritas compiles Swift faster. The NowLookHere files remain untouched; any such change would need its own scoped task.

## How this baseline was used

The initial device-picker-only suggestion was too small to test the user's north star. The chosen follow-up instead implemented the full build/install/launch decision flow in Veritas source and exercised it against fake system commands. That made Xcode's dominant runtime a secondary measure while testing whether the language could own orchestration, JSON interpretation, process arguments, error handling, and cleanup. See [the vertical-proof results](veritas_phone_deploy_vertical_proof_results_v0_1.md).
