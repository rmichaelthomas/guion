# Field Kit Log

A local equipment inspection log. Each inspection uses the current local calendar date automatically; the host process time zone controls the date. Data is stored in SQLite using native Veritas services. No Python, external package, or network service is required.

## Install and choose a data directory

Use the installed Veritas CLI to package and install the source:

    veritas package app --output field-kit-package
    veritas verify-package field-kit-package
    veritas install field-kit-package --prefix /your/apps/field-kit

Create or choose an existing writable data directory outside the runtime and app installation. Copy `sample.toml` into that directory as `operator.toml`. Its `database.path` selects the database filename:

    [database]
    path = "field-kit.sqlite"

Set `VERITAS_FILE_ROOT` to the absolute path of that directory on each launch:

    export VERITAS_FILE_ROOT="/your/data/Field team"
    "/your/apps/field-kit/bin/field-kit-log" operator.toml add meter-1 "Field meter"
    "/your/apps/field-kit/bin/field-kit-log" operator.toml inspect meter-1 pass "Calibration OK"
    "/your/apps/field-kit/bin/field-kit-log" operator.toml inspect meter-1 fail "Case cracked"
    "/your/apps/field-kit/bin/field-kit-log" operator.toml list
    "/your/apps/field-kit/bin/field-kit-log" operator.toml list --failed-only

The config filename is relative to the data directory. The directory must already exist. The database is created on first successful use. Use relative database paths within the granted directory; this installed runtime rejects absolute SQLite paths despite the host API documentation stating that absolute paths are required. Parent traversal is rejected by the runtime. The sample uses a filename directly in the root so no further directory creation is needed.

## Commands and output

`add ID NAME` creates an item. IDs and names must be nonempty; duplicate IDs are errors. `inspect ID pass|fail NOTE` records an inspection for a known item, and accepts a note containing 1 to 500 characters. Quote names and notes containing spaces. No date argument is accepted. `list` prints every inspection, and `list --failed-only` prints failures only. Both use insertion sequence order, including multiple inspections on the same date.

History is tab-separated: sequence, item ID, ISO local date, result, note. Text is printed literally, so use single-line notes without tabs for readable terminal output. Commands return zero on success and nonzero on failure; usage and validation errors return 2. Diagnostics go to stderr. Duplicate IDs, unknown items, invalid results, and invalid arguments preserve prior data. Native SQLite transactions coordinate writers and roll back unfinished operations.

## Move and maintain

Move the whole installation prefix together; its launcher locates the versioned app relative to itself. Spaces and Unicode are supported in installation and data-directory paths. Keep the database directory separate and continue setting `VERITAS_FILE_ROOT` to it. The manifest explicitly grants clock access with `[clock] enabled = true`. The source imports `std:time` and calls `today_local()` for each new inspection.

Stop writers before making a backup of the data directory. Retain `operator.toml` and the SQLite database together. The final delivered evidence includes source before and after the failed-only change, acceptance commands, captured output, and SHA-256 checks proving rejected operations preserved the database bytes. The final evidence database includes two items and three inspections; the third was deliberately recorded under `TZ=Pacific/Kiritimati` to prove local timezone handling.

The acceptance scripts preserve the original builder paths as an audit record; they expect the staged database state and should not be rerun against production data.
