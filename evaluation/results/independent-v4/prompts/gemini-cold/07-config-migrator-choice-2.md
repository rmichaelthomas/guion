You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 07-config-migrator-choice-2
Schedule SHA-256: 6a070a9801ffb2479d16ab2b3ddd9769cd74235050d8c48a03a09bf77dc9c8dd
Schedule file SHA-256: d3ff11c45c77160349cc998ba9db7ca85f28734a1137ec5cf9a7e78cb9fdee9a
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: Google Gemini gemini-4-argon (gemini-4-argon); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Versioned configuration migrator

**Class:** package/library or maintainable application
**Origin:** independent-candidate brief draft
**Initial request:** Create a small multi-module command-line application. `config-migrator check --file PATH` validates the requirements for the document's declared schema version. `config-migrator migrate --file PATH` advances exactly one schema version atomically: schema 1 to schema 2, or schema 2 to schema 3; schema 3 is a no-op. Only integer versions 1, 2, and 3 are supported; versions below 1 or above 3 are actionable errors and must not replace the original file. Schema 1 requires `version = 1` and a non-empty string `name`. Schema 2 requires `version = 2`, a non-empty string `name`, and a `[sync]` table with positive integer `timeout_seconds`; migrating schema 1 adds `timeout_seconds = 30`. Schema 3 additionally requires positive integer `parallelism`; migrating valid schema 2 adds `parallelism = 4`. A schema 2 file missing or containing an invalid `timeout_seconds` is invalid, and migration must fail without changing it. Booleans are not integers for these fields. Other sections and values are preserved. `[sync]` is reserved for migration fields: if schema 1 contains `[sync]`, it must be a TOML table; any other value is an actionable error and migration leaves the source byte-for-byte unchanged. When it is a table, preserve its values and comments, adding migration fields only when absent. Migration fails without changing the source when an existing reserved value has an invalid type. Migration is a no-op for the current schema. Preserve unrelated values and comments. Invalid types must not replace the original file.

**Follow-up change:** Add `sync.parallelism = 4` as a schema 3 default and migrate schema 2 to 3 while preserving explicit values. For schema 2, `parallelism` is optional; if present it must be a positive integer. `check` accepts it when absent, and migration adds `parallelism = 4`. If the value is present but invalid, both check and migrate must fail without changing the file bytes.

**Acceptance:** Current version no-op; each supported old version; unsupported future version; invalid types; preservation of standalone and inline comments; atomic failure; follow-up default and explicit-value preservation; clean install from a fresh project; no implicit system-Python dependency.

**Fairness notes:** Python may use a maintained TOML/comment-preserving library. The non-Python candidate may use its normal dependency/interop path. Capture dependency resolution, startup, package size, and API usability.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
