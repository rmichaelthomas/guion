You are advising a developer choosing a language for the attached task.
Do not implement the task, browse, use tools, or infer results from code that has not been built.
Choose exactly one: Guion, Python, or either. Explain the decisive task-specific tradeoffs and what would change your choice.
In the product-information arm use only the supplied profiles. In the cold arm no product documentation is supplied; use only prior knowledge and the task brief.
Do not make a universal language-replacement claim.

Assignment ID: 08-documentation-link-index-choice-1
Schedule SHA-256: 6a070a9801ffb2479d16ab2b3ddd9769cd74235050d8c48a03a09bf77dc9c8dd
Schedule file SHA-256: d3ff11c45c77160349cc998ba9db7ca85f28734a1137ec5cf9a7e78cb9fdee9a
Choice-safe bundle SHA-256: 4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945
Presentation order: Python, Guion
Configured assistant: Google Gemini gemini-4-argon (gemini-4-argon); reasoning effort provider-default
Budget: 5 active minutes and 0 tool calls.
No implementation results, benchmarks, author calibration, or candidate runs have been shown.

# Task brief
# Local documentation link index

**Class:** package/library or maintainable application
**Origin:** independent-candidate brief draft; revised after overlap audit
**Initial request:** Create a multi-module CLI for a folder of Markdown documentation. `doc-index build --root PATH --database PATH` indexes UTF-8 Markdown files and local `[[Page title]]` references in SQLite. `doc-index check --database PATH --json PATH` writes `{"missing":[{"source":"relative/path","target":"title"}],"ambiguous":[{"source":"relative/path","target":"title"}]}`; each issue object contains only `source` and `target`. Reporting findings exits 0. `doc-index backlinks --database PATH --json PATH TITLE` writes `{"backlinks":["relative/path"]}` with source pages whose link target matches the normalized query term. Normalize titles by collapsing consecutive Unicode whitespace to one ASCII space and applying Unicode case-folding. Sort document/source relative paths by that same normalization, with the original relative-path UTF-8 bytes as the tie-breaker. Sort issue rows by normalized source path, original source-path UTF-8 bytes, normalized target, then original target UTF-8 bytes; sort backlinks by the same source-path key. Ignore symlinked files and directories. If a Markdown file is not valid UTF-8, fail without replacing the existing index. The task does not promise a distinct per-file permission-denied behavior.

**Follow-up change:** Accept optional front matter only when the first line is exactly `---` and a later line is exactly `---`. If the first line is not exactly `---`, treat the document as having no front matter and ignore any `aliases:` text in its body. If the first line is exactly `---` but no later exact `---` delimiter exists, also treat the entire document as ordinary Markdown with no front matter; ignore any `aliases:` text in it. Within valid front matter, accept at most one `aliases` field, written on one line as a JSON array of strings (for example, `aliases: ["Coffee", "R&D, Inc."]`). JSON string escaping applies; `aliases: []` means no aliases. Reject malformed JSON, duplicate aliases fields, non-string values, and nested arrays; ignore other front-matter fields. Resolve links by either page title or alias for missing/ambiguous checks. Alias normalization matches title normalization. Multiple matching names that belong to the same page count as one target for ambiguity; names matching different pages are ambiguous. A backlinks query matches only links whose normalized target equals the normalized query term; it does not expand to other names for the same page and still returns exact-text source links when that destination name is ambiguous. A successful build replaces the index with the complete set of current documents and links, removing stale rows; a failed build leaves the previous index intact. The title is the first level-one Markdown heading, or the filename stem when none exists. Check and backlinks JSON retain the exact initial-stage shapes.

**Acceptance:** Nested folders; Unicode titles; duplicate title ambiguity; missing links; case/whitespace normalization; filename-stem fallback; symlink boundary; invalid UTF-8 and malformed alias metadata preserve prior index; deterministic output; alias matches including escaped punctuation and collisions; successful rebuild removes deleted documents and stale links; clean-workspace builds with fresh databases.

**Fairness notes:** Python may use its standard-library SQLite module. The task tests a filesystem index and parser rather than the existing task-ledger CRUD/state workflow; this replaces the earlier local issue-queue draft, which overlapped that known development example.

# Option 1: Python
No product profile or documentation is supplied for this option.

# Option 2: Guion
No product profile or documentation is supplied for this option.
