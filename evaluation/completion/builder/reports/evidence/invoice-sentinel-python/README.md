# Invoice Sentinel (Python)

Requires Python 3.14.7; uses only the standard library. No installation step.

```sh
python3 /absolute/path/invoice_sentinel.py \
  --input /absolute/path/ledger.tsv \
  --as-of 2026-02-01 \
  --output /absolute/path/report.json

python3 /absolute/path/invoice_sentinel.py \
  --input /absolute/path/ledger.tsv \
  --as-of 2026-02-01 \
  --output /absolute/path/report.json \
  --vendor 'Café Supplies'
```

The input is UTF-8 TSV (an optional UTF-8 BOM is accepted), with this exact header:

```text
invoice_id<TAB>vendor<TAB>amount_cents<TAB>due_date<TAB>state
```

There is one physical line per invoice, with five fields; TSV quoting and embedded
newlines are not supported. IDs and vendors must be nonempty. Amounts are signed
base-ten integer cents, including negative credit amounts, with no whitespace,
fractional notation, or scientific notation. Dates must be valid calendar dates
in exact YYYY-MM-DD form. State must be `open` or `paid`.

A due date strictly before the as-of date is overdue. Vendors appear in Unicode
code-point order, including vendors with only paid invoices and zero open totals.
JSON object keys are sorted, UTF-8 names are preserved, and output has a final
newline. Rejected rows appear in input order with physical line number and reason,
and are also reported on stderr. Valid rows still contribute. Rejected amounts
and dates never contribute to totals.

With `--vendor`, matching is exact and case sensitive. Only the selected vendor
appears; `excluded_row_count` counts otherwise-valid invoices from other vendors,
including paid invoices. Rejected rows are validated and reported regardless of
filter. Without `--vendor`, there is no excluded-row field and original output
bytes are preserved. A nonexistent vendor returns an empty vendor list.

All command arguments and the whole input are validated/read before creating the
replacement output. Invalid command dates, unreadable input, invalid headers, or
serialization failures return status 2 and preserve an existing destination.
Malformed ledger rows are rejected individually and included in the successful
audit output. Successful output is written to a temporary file alongside the
destination, flushed, then atomically replaces it. The output parent directory
must already exist. Input and output must resolve to different paths.

## Evidence

- `invoice_sentinel_initial.py`: preserved initial implementation.
- `invoice_sentinel.py`: completed follow-up implementation.
- `fixtures/`: synthetic inputs.
- `checks.py`: acceptance runner; uses a separate working directory and copies
  the source and fixtures into `relocation/Factures Café 🧾`.
- `initial-results.json`, `followup-results.json`: individual checks and timings.
- `initial-commands.jsonl`, `followup-commands.jsonl`: every launched tool command,
  current directory, exit status, stdout, stderr, and wall time.
- `report.json`: final observed result and recommendation.

Checks used:

```sh
python3 /private/tmp/veritas-fresh-builders/invoice-sentinel-python/checks.py initial
python3 /private/tmp/veritas-fresh-builders/invoice-sentinel-python/apply_followup.py
python3 /private/tmp/veritas-fresh-builders/invoice-sentinel-python/checks.py followup
```

The `initial` phase must be run against the initial source; that source is retained
as `invoice_sentinel_initial.py`. The follow-up source is now the default.
