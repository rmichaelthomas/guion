You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 03-release-notes-choice-1
Schedule SHA-256: 4630653714bd96f277e70ae17ccc8e37a1ee64ad37cf1d547a7c92494bd53bc1
Schedule file SHA-256: 1398aa6edacdc22d115a7597209e19ad1d60b624c69200fa49edf0c19cb71d4e
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Guion, Python
Configured assistant: OpenAI Codex gpt-6.1-sol (gpt-6.1-sol); reasoning effort medium
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Release note assembler

**Class:** small automation or data CLI
**Origin:** independent-candidate brief draft
**Initial request:** Build a CLI invoked as `release-notes --changes DIR --config PATH --markdown PATH --summary PATH`. The config must be a JSON object with exactly `date` (a Gregorian date written `YYYY-MM-DD`) and `category_order` (a non-empty array of unique non-empty strings). Each change file must be named `<SemVer>.json`; its JSON object must contain exactly non-empty string fields `id`, `category`, and `text`. The version is taken from the filename stem. Duplicate IDs are rejected across the entire input directory. Versions use SemVer 2.0.0 syntax and precedence; build metadata does not affect precedence. For equal precedence, sort the original version strings in ascending ASCII bytewise order. Reject malformed versions and records whose category is absent from `category_order`. Validate records, group entries by category, sort newest-first and entries by ID, and write Markdown release notes. In change text, prefix every ASCII punctuation character with a backslash. This means characters in code-point ranges U+0021–U+002F, U+003A–U+0040, U+005B–U+0060, and U+007B–U+007E; escape an existing backslash exactly once. Do not alter letters, digits, or spaces. Emit only configured categories containing entries. Write a JSON summary shaped as `{"releases":[{"version":"1.2.0","entry_count":1}]}` with releases in the same order. Invalid records identify the source file and field and leave both existing outputs untouched. Each output is atomically replaced after validation; two separate paths are not a crash-atomic transaction.

The Markdown heading format is `# Release notes (DATE)`, then for each version `## VERSION`, then each configured category as `### CATEGORY`, followed by `- TEXT` entries. Category names and change text must be non-empty single-line strings; reject either carriage return (CR) or line feed (LF) in either field. Separate headings/entries with one blank line and end the file with one newline.

**Follow-up change:** Add `--since VERSION`; include only releases newer than that version while retaining the configured category order and report format. Validate every input filename, record, category, and ID across the entire directory before filtering rendered releases; malformed excluded records and duplicate IDs across the filter boundary still fail while preserving both outputs. Invalid version syntax must be a clear error.

**Acceptance:** Multiple files and categories; duplicate IDs; malformed dates and filename versions; prerelease before its final version; build-metadata precedence ties; unconfigured categories rejected; stable ordering independent of filesystem order; Markdown escaping for punctuation; missing `--since` compatibility; boundary versions; both existing outputs remain unchanged on input validation or rendering failure; clean-workspace delivery. Do not claim crash-atomic replacement across both output paths.

**Fairness notes:** No Git history is needed. Python may use the standard library. Record any external package and its install footprint if chosen.

# Option 1: Guion
No product profile or documentation is supplied for this option.

# Option 2: Python
No product profile or documentation is supplied for this option.
