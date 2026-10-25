# Assignment: Invoice Sentinel

**Target class:** small automation/data CLI

Build a command-line tool that audits a tab-separated vendor invoice ledger. Each non-header row has `invoice_id`, `vendor`, `amount_cents`, `due_date` (`YYYY-MM-DD`), and `state` (`open` or `paid`). Given an input file, an `as-of` date, and an output path, produce deterministic JSON with overdue open invoice count and cents by vendor, total open cents by vendor, and a rejected-row count. A malformed row must be reported with its line number and reason while valid rows still contribute. Do not use floating point for money. Output ordering must be deterministic.

**Follow-up change:** add an optional exact `--vendor` filter. When present, report only that vendor and include the number of otherwise-valid rows excluded by the filter. Existing behavior without the option must remain unchanged. Invalid dates or amounts must be rejected before replacing an existing output.

**Acceptance:** exercise multiple vendors, paid and open rows, overdue and future dates, malformed rows, an absent vendor filter, an exact vendor filter, deterministic repeat output, and preservation of an existing output after invalid arguments. Deliver source, a short usage note, and fixtures. Run the initial and follow-up checks from a different working directory after copying the project to a Unicode-and-spaces path. No third-party dependencies.
