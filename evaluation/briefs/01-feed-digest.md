# Acceptance brief: Feed digest

Build a command-line program that accepts two or more RSS/Atom feed URLs, fetches them concurrently, parses them with the managed Python `feedparser` library, deduplicates entries by canonical URL, sorts entries deterministically, and writes `digest.md`. Preserve Unicode titles. Duplicate metadata resolution must be independent of input URL order: prefer the record with a published date; when both records either have or lack one, prefer the lexicographically earlier title. A failed feed must be reported with its URL and stable error code while successful feeds still contribute entries.

The program must be understandable from the shipped language reference and executable without a global Python/uv installation after its managed environment has been synchronized. The Python comparison receives the same URLs, feed responses, package version, byte limits, timeout and output contract.

## Acceptance cases

- Two valid feeds containing a shared link produce one entry for that canonical link.
- Non-ASCII titles survive fetch, Python parsing, Veritas/Python conversion and output.
- Input order does not change the output; entries sort by canonical link.
- A malformed feed produces no corrupt entries.
- One failing HTTP status is reported and does not discard entries from another feed.
- A follow-up feed with a changed title is reflected after a fresh run.
- An ungranted host is denied and reported without being contacted.

This is a known development case, not an independent recommendation evaluation.
