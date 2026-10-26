You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 09-lockfile-auditor-choice-1
Schedule SHA-256: 4630653714bd96f277e70ae17ccc8e37a1ee64ad37cf1d547a7c92494bd53bc1
Schedule file SHA-256: 1398aa6edacdc22d115a7597209e19ad1d60b624c69200fa49edf0c19cb71d4e
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: OpenAI Codex gpt-6.1-sol (gpt-6.1-sol); reasoning effort medium
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Lockfile policy auditor

**Class:** package/library or maintainable application
**Origin:** independent-candidate brief draft
**Initial request:** Implement a multi-module CLI invoked as `lockfile-auditor --lockfile PATH --policy PATH --output PATH`. A lock document is an object with a `packages` array; each entry requires non-empty string `name` and SemVer 2.0.0 `version`, and may have string `source` and `checksum` fields. Missing, null, or empty source/checksum count as absent when required. Multiple lock entries may share a package name; evaluate each entry independently and retain repeated diagnostics when separate entries trigger the same rule. Policy is an object whose `deny` (string array), `minimum_versions` and `maximum_versions` (name-to-SemVer maps), and Boolean `require_source`/`require_checksum` fields are optional and default to `[]`, `{}`, `{}`, `false`, and `false`, respectively. Never install or execute dependencies. Emit `{"violations":[{"name":"blocked-lib","rule":"denied"}],"warnings":[]}` with both arrays sorted by package name then rule. Minimum and maximum version bounds are inclusive. Malformed input is an actionable nonzero error and leaves existing output untouched.

**Follow-up change:** Add `--as-of DATE` and `exceptions` entries with exactly two fields: `{"name":"PACKAGE","expires":"YYYY-MM-DD"}`. `--as-of` and exception dates must be valid Gregorian dates written exactly as `YYYY-MM-DD`; an explicitly supplied empty `--as-of` value is invalid and is not equivalent to omitting the option. Names must be non-empty and unique. A non-empty exceptions list requires `--as-of`. An exception suppresses only that package's deny rule when `as_of < expires`; on or after expiration, add `{"name":"PACKAGE","rule":"expired_exception"}` to `warnings` and keep the deny violation in `violations`. Exceptions do not suppress version/source/checksum rules. Invalid exception fields, exception dates, or `--as-of` values fail without replacing an existing report. Keep original behavior when no exceptions are present.

**Acceptance:** Valid and malformed lockfiles; missing version and wrong field types; multiple versions and sources; checksum missing/present; policy boundaries; deterministic full diagnostics; exception for denied and non-denied package; duplicate exception rejection; valid/on-expiry/expired behavior; exit codes; fixture with enough entries to measure meaningful-volume processing; source-free package; clean install.

**Fairness notes:** Freeze a documented lockfile subset and parser fixture before evaluation. Python may use its standard library or packages already present in the shared offline wheelhouse listed in `evaluation/sandbox/dependencies/requirements.txt`. No package may be downloaded or installed from another source. The non-Python candidate may implement the feature directly or use its declared managed Python bridge with that same wheelhouse; additional package downloads are forbidden. Compare the same inputs, policy semantics, and packaging expectations.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
