**Choose python for this task.**

The decisive factor is the auditor’s concentration of strict input validation and policy edge cases: full SemVer precedence, exact Gregorian dates, omitted versus explicitly empty options, repeated diagnostics, and preserving an existing report on failure. Python’s standard library supplies CLI parsing, JSON handling, calendar validation, sorting, and file replacement primitives, making it a direct fit for a maintainable multi-module CLI.

SemVer still needs deliberate treatment: use a suitable package only if it appears in the supplied offline dependency lock, or implement the specified SemVer rules directly. Neither ordinary string comparison nor a general package-version comparator should be assumed to satisfy SemVer 2.0.0.

Veritas has relevant documented capabilities: modules, records, typed results, JSON parsing, atomic replacement, and source-free application bundles. Its packaging workflow is a meaningful advantage. However, the supplied profile also describes pre-1.0 compatibility limits, restricted matching and collection capabilities, and runtime errors for typed JSON access failures. Those introduce additional implementation and maintenance considerations for this validation-heavy application. Using its Python bridge is allowed, but adds a bridge and managed runtime boundary to logic that can run directly in Python.

Python still must deliver a locked project and a wheel or zipapp that passes relocation with the original source tree absent. Its packaging success is not established by this recommendation.

I would change the recommendation toward **veritas** if integration into an existing Veritas application, its runtime limits, or its bundled delivery and integrity metadata became primary requirements—and the supported Linux ARM64 target remained sufficient. No supplied evidence establishes a performance winner; meaningful-volume processing and clean delivery require verification for either choice.
