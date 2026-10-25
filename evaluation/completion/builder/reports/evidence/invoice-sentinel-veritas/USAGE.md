# Invoice Sentinel (Veritas)

Run the packaged CLI from a directory containing the ledger:

    /path/to/final-app/run --input ledger.tsv --as-of 2026-01-15 --output report.json
    /path/to/final-app/run --input ledger.tsv --as-of 2026-01-15 --output report.json --vendor Acme

`initial-app/run` is the initial revision without `--vendor`. `project/main.vr` and `final-source.vr` are the final revision. Input and output paths are relative to the caller directory, within the granted file root. Override the root with VERITAS_FILE_ROOT. Source can be built with the installed Veritas `package` command using `project/`.

The header must be `invoice_id`, `vendor`, `amount_cents`, `due_date`, `state` separated by tabs. Money uses arbitrary-precision, nonnegative decimal integer cents. Dates are strict Gregorian YYYY-MM-DD (years 0001–9999). Overdue means due date strictly before the as-of date. Vendors with valid paid rows are retained with zero totals. Malformed rows are excluded, reported with one-based physical line number and reason in JSON and stderr, and do not prevent valid rows contributing. A final newline does not add a rejected row.

The JSON has `as_of`, `vendors`, `rejected_row_count`, and `rejected_rows`. Each vendor summary has `open_cents`, `overdue_open_cents`, `overdue_open_count`. With `--vendor`, exact Unicode text comparison selects one vendor and adds `excluded_valid_row_count`; malformed rows remain in rejection details. With no filter, the initial JSON shape is preserved. Vendor object ordering and repeat bytes are deterministic.

Invalid command arguments, invalid as-of dates, header/read errors, and unknown options return nonzero before replacing output. Invalid ledger amounts/dates are malformed rows, per the requirement that valid rows still contribute; rejection and validation finish before the single atomic output replacement. There is no separate CLI amount argument. Atomic output replacement does not coordinate simultaneous writers.

`checks.py PACKAGE_DIRECTORY MODE` runs acceptance with the standard library Python test driver; the application itself is pure Veritas and never launches Python. Tests run in directories separate from each package. Relocated checks used `/private/tmp/veritas-fresh-builders/Invoice Sentinel 雪 relocation` and both saved revisions. `measurements.json` contains the timed build, verification, installed launch, and artifact sizes.
