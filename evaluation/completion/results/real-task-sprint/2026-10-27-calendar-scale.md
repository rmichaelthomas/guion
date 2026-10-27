# Calendar event scale results

**Implementation commit:** `214bdb8` (`perf(calendar): prune disjoint feed ranges`)

## What changed

Each fetched calendar now retains its parsed events and tracks the earliest start and latest end of events that survive the existing filters and requested time window. If two calendars' time ranges cannot overlap, Guion skips building a second event list, sorting it, and checking event pairs. If ranges might overlap, it still builds the rows and runs the complete conflict path.

The application limits were not changed. The fast path is safe because disjoint event-time ranges cannot contain a cross-calendar conflict; intersecting ranges continue through the full logic.

## Frozen task acceptance

The existing calendar-conflicts suite passed **11/11 cases and 40/40 checks** in both Guion and Python. It includes actual conflicts, clipped boundaries, time zones, malformed events, timeouts, and preservation of an existing report on invalid input.

## Large-feed comparison

The synthetic scale case used three calendars with **4,500 non-overlapping events each** (13,500 total), plus the existing malformed and unavailable feed cases. It retained concurrent requests and output validation. Fixture setup was excluded. Each candidate ran in a fresh process with Guion's source cache bypassed; the operating-system file cache was uncontrolled.

Seven randomized paired trials measured:

| Candidate | Median runtime |
| --- | ---: |
| Guion | 316.1 ms |
| Python | 11.957 s |

The Guion/Python median ratio was **0.0264** (95% bootstrap interval **0.0243–0.0266**), about **37.9× faster** for this specific disjoint-range workload. Every Guion and Python run passed the expected-output and concurrency checks. The [machine-readable results](2026-10-27-calendar-scale.json) contain all samples and checks.

A three-pair screen at 4,000 events per calendar measured 291.7 ms for Guion and 9.483 s for Python (ratio 0.0306). At 5,000 events per calendar (15,000 total), Guion reached the existing `R_STEP_LIMIT` while processing calendar requests; Python passed. That single boundary attempt is recorded in the [5,000-event probe](2026-10-27-calendar-scale-5000-boundary.json). The default instruction budget remains unchanged.

## Limits of this result

This measures a real calendar application on synthetic, non-overlapping event ranges. It demonstrates an application-level shortcut for feeds whose time ranges are disjoint. It does not establish a 37.9× general language speed advantage, and it does not measure dense conflict generation. Calendars with intersecting ranges use the complete conflict-detection path and remain the next workload to examine.

The requested agent-preference rerun did not complete. The automatic approval reviewer blocked the temporary Codex evaluator because its local process attempted to access the private Codex state database; the earlier evaluation approval did not cover that file. No new choice is claimed. The latest completed preference result remains 3/3 for Guion.
