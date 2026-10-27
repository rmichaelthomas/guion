# Calendar conflict detection: interleaved scale

## Plain-English result

Guion now handles **8,000 events in each of three interleaved calendars**—24,000 events total—in a fresh source run. In the selected synthetic case, Guion took **1.230 seconds** and Python took **37.671 seconds**, about **30.3× faster**. Every trial produced the expected output.

| Events per calendar | Paired trials | Guion median | Python median | Speed ratio | 95% interval for Guion/Python |
|---:|---:|---:|---:|---:|---:|
| 4,000 per calendar (12,000 total) | 7 | 0.499 s | 9.663 s | 19.3× | 0.0485–0.0548 |
| 6,000 per calendar (18,000 total) | 3 | 0.853 s | 22.551 s | 26.4× | 0.0376–0.0462 |
| 8,000 per calendar (24,000 total) | 3 | 1.230 s | 37.671 s | 30.3× | 0.0324–0.0331 |

The result is specific to this calendar workload. Events are sorted within each feed and interleaved across feeds, with one-second events staggered so there are no actual cross-calendar conflicts. The case still exercises calendar parsing, filtering, merging and the no-conflict scan; its two fixed HTTP endpoints also continue to check error handling and concurrency. The separate frozen calendar suite passes **11/11 cases and 40/40 checks in both Guion and Python**, including overlapping-event cases.

## Resource limit and method

The calendar project declares `instruction_limit = 10000000` in its own `guion.toml`; the language-wide default remains **2,000,000**. With the default limit, the same 4,000-event-per-calendar workload reached `R_STEP_LIMIT`; the project-specific budget allowed the larger case to complete without changing the global default. The default-limit boundary probe is saved separately.

Measurements use fresh processes, bypass Guion's source cache, randomize paired order, verify expected outputs on every run, and exclude fixture setup. The operating system's file cache was uncontrolled. The 4,000-event case used seven paired trials; the 6,000- and 8,000-event scaling cases used three each, so those larger points are exploratory scale checks rather than a broad performance guarantee.

These measurements do not establish that Guion is generally faster than Python. They show that this specific calendar implementation completes the tested workloads within its declared project budget and is faster in these runs.

## Evidence

- [4,000 events per calendar](2026-10-27-calendar-interleaved-4000.json)
- [6,000 events per calendar](2026-10-27-calendar-interleaved-6000.json)
- [8,000 events per calendar](2026-10-27-calendar-interleaved-8000.json)
- [4,000-event default-budget boundary probe](2026-10-27-calendar-interleaved-4000-default-budget-boundary.json)
