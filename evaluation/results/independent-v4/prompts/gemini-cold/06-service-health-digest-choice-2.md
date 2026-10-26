You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 06-service-health-digest-choice-2
Schedule SHA-256: 6a070a9801ffb2479d16ab2b3ddd9769cd74235050d8c48a03a09bf77dc9c8dd
Schedule file SHA-256: d3ff11c45c77160349cc998ba9db7ca85f28734a1137ec5cf9a7e78cb9fdee9a
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: Google Gemini gemini-4-argon (gemini-4-argon); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Service health digest

**Class:** concurrent network or package integration
**Origin:** independent-candidate brief draft
**Initial request:** Build a CLI invoked as `service-health --config PATH --json PATH --text PATH --concurrency N --timeout-ms N`. RFC3339 timestamps use the exact `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` form with a valid Gregorian date, seconds from 00 through 59, a timezone offset, and (when present) one to six fractional digits; fractions longer than six digits are invalid. The config JSON is `{"services":[{"name":"api","url":"..."}]}`. Query configured endpoints concurrently. A 2xx JSON response with `{"status":"ok"}` is healthy; any other valid JSON status is unhealthy; malformed JSON is invalid_response; a request exceeding timeout is timeout. Each request has one total wall-clock deadline from connection start through the complete response body; slow trickle data does not reset it. A non-2xx HTTP response is unhealthy with `raw_status` null, and its body is not parsed. Continue after endpoint failures. Emit JSON `{"results":[{"service":"name","status":"healthy|unhealthy|timeout|invalid_response|maintenance","raw_status":"value or null"}]}` sorted by service name, and a text digest with exactly one `SERVICE STATUS` line per result in the same order. Endpoint configuration is supplied at runtime; do not hardcode fixture addresses.

**Follow-up change:** Add `--now RFC3339` and optional per-service `maintenance` objects containing inclusive `start` and `end` RFC3339 instants using the timestamp syntax above. Service names must be unique, non-empty strings. If any service config contains maintenance, `--now` is required; without configured maintenance, `--now` is optional and has no effect. Each maintenance object must contain valid `start` and `end` instants with start <= end. A request timeout remains timeout, including during a maintenance window. A non-2xx HTTP response is unhealthy with `raw_status` null and is not parsed. For 2xx responses, malformed JSON, a non-object JSON value, or a missing/non-string status is invalid_response with `raw_status` null. A string status equal to `ok` is healthy; any other string is unhealthy and retained in `raw_status`. During the window, map unhealthy and invalid_response results to maintenance while preserving their pre-maintenance `raw_status`; healthy results remain healthy. Outside the window, preserve the ordinary status.

**Acceptance:** Mixed healthy and failed services; slow endpoint; timeout; malformed JSON; concurrency cap; per-request timeout bound; empty configuration; maintenance boundaries and timezone; deterministic service ordering; no leaked credentials; clean offline fixture run and packaged launch. `start` and `end` are both inside the maintenance window. Empty configuration writes the documented empty result and an empty text file. Invalid maintenance timestamps must fail without replacing either existing output file.

**Fairness notes:** The harness supplies a loopback fixture and equivalent network grants. Python may use its standard library or packages already present in the shared offline wheelhouse listed in `evaluation/sandbox/dependencies/requirements.txt`. No package may be downloaded or installed from another source. The non-Python candidate may implement the feature directly or use its declared managed Python bridge with that same wheelhouse; additional package downloads are forbidden. Record dependencies, setup time, and package size.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
