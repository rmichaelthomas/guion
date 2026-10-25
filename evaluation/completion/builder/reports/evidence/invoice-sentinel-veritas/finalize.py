import json, pathlib, shutil, subprocess, datetime
r=pathlib.Path(__file__).parent
(r/'fixtures').mkdir(exist_ok=True)
shutil.copy2(r/'checks-initial/ledger.tsv',r/'fixtures/ledger.tsv')
shutil.copy2(r/'initial.vr',r/'initial-source.vr')
shutil.copy2(r/'project/main.vr',r/'final-source.vr')
shutil.copy2('/private/tmp/veritas-fresh-builders/contracts/invoice-sentinel-veritas.limn',r/'session-contract.limn')
rel=pathlib.Path('/private/tmp/veritas-fresh-builders/Invoice Sentinel 雪 relocation')
for name in ['commands-relocated-initial.json','commands-relocated-followup.json','results-relocated-initial.json','results-relocated-followup.json']:
 shutil.copy2(rel/name,r/name)
(r/'USAGE.md').write_text('''# Invoice Sentinel (Veritas)

Run the packaged CLI from a directory containing the ledger:

    /path/to/final-app/run --input ledger.tsv --as-of 2026-01-15 --output report.json
    /path/to/final-app/run --input ledger.tsv --as-of 2026-01-15 --output report.json --vendor Acme

`initial-app/run` is the initial revision without `--vendor`. `project/main.vr` and `final-source.vr` are the final revision. Input and output paths are relative to the caller directory, within the granted file root. Override the root with VERITAS_FILE_ROOT. Source can be built with the installed Veritas `package` command using `project/`.

The header must be `invoice_id`, `vendor`, `amount_cents`, `due_date`, `state` separated by tabs. Money uses arbitrary-precision, nonnegative decimal integer cents. Dates are strict Gregorian YYYY-MM-DD (years 0001–9999). Overdue means due date strictly before the as-of date. Vendors with valid paid rows are retained with zero totals. Malformed rows are excluded, reported with one-based physical line number and reason in JSON and stderr, and do not prevent valid rows contributing. A final newline does not add a rejected row.

The JSON has `as_of`, `vendors`, `rejected_row_count`, and `rejected_rows`. Each vendor summary has `open_cents`, `overdue_open_cents`, `overdue_open_count`. With `--vendor`, exact Unicode text comparison selects one vendor and adds `excluded_valid_row_count`; malformed rows remain in rejection details. With no filter, the initial JSON shape is preserved. Vendor object ordering and repeat bytes are deterministic.

Invalid command arguments, invalid as-of dates, header/read errors, and unknown options return nonzero before replacing output. Invalid ledger amounts/dates are malformed rows, per the requirement that valid rows still contribute; rejection and validation finish before the single atomic output replacement. There is no separate CLI amount argument. Atomic output replacement does not coordinate simultaneous writers.

`checks.py PACKAGE_DIRECTORY MODE` runs acceptance with the standard library Python test driver; the application itself is pure Veritas and never launches Python. Tests run in directories separate from each package. Relocated checks used `/private/tmp/veritas-fresh-builders/Invoice Sentinel 雪 relocation` and both saved revisions. `measurements.json` contains the timed build, verification, installed launch, and artifact sizes.
''')
measure=json.loads((r/'measurements.json').read_text())
start=datetime.datetime.fromisoformat('2026-10-25T09:10:43+00:00')
setup_end=datetime.datetime.fromtimestamp((r/'project').stat().st_birthtime,datetime.timezone.utc)
initial_end=datetime.datetime.fromtimestamp((r/'commands-initial.json').stat().st_mtime,datetime.timezone.utc)
followup_end=datetime.datetime.fromtimestamp((r/'commands-followup.json').stat().st_mtime,datetime.timezone.utc)
finish=datetime.datetime.now(datetime.timezone.utc)
report=dict(schema_version=1,status='complete',assignment_id='invoice-sentinel-veritas',language='veritas',brief_id='invoice-sentinel',start_utc=start.isoformat(),finish_utc=finish.isoformat(),initial_correct=True,followup_correct=True,relocation_passed=True,setup_seconds=(setup_end-start).total_seconds(),build_seconds=(initial_end-setup_end).total_seconds(),followup_seconds=(followup_end-initial_end).total_seconds(),installed_launch_seconds=measure['commands'][2]['seconds'],artifact_bytes=measure['sizes']['artifact_bytes'],interventions=['No external packages or network requests used. Python3 standard library used only for edit, measurement and acceptance harnesses; delivered app is pure Veritas.','The bare veritas --help invocation was unsupported; used the installed README/reference instead.','setup/build/followup phase durations use UTC start plus filesystem creation/modification timestamps; measurements.json separately records a directly timed final compilation.','Invalid ledger dates/amounts are rejected rows while valid rows contribute. Invalid CLI dates/options preserve an existing report. This resolves the brief wording consistently with its malformed-row requirement.'],failures=['Installed CLI does not support --help (exit 1); documented command reference supplied the commands.'],recommendation=dict(preferred_language='undetermined',reason='The isolated Veritas implementation passed the required cases. This assignment did not inspect or execute a Python implementation, so it does not establish a comparative preference.',observed_user_benefit='Exact arbitrary-precision cents, deterministic JSON, row diagnostics, atomic output and a relocatable Python-free app were delivered with no third-party dependencies.',material_regressions=['The packaged app occupies 12881497 bytes, including its runtime.','One measured installed launch took about 0.462 seconds.','Strict calendar validation and flag parsing were implemented manually.']),evidence_paths=[str(p) for p in [r/'USAGE.md',r/'initial-source.vr',r/'final-source.vr',r/'project',r/'fixtures',r/'initial-app',r/'final-app',r/'installed',r/'checks.py',r/'measurements.json',r/'commands-initial.json',r/'commands-followup.json',r/'commands-relocated-initial.json',r/'commands-relocated-followup.json',r/'results-initial.json',r/'results-followup.json',r/'results-relocated-initial.json',r/'results-relocated-followup.json',rel,r/'session-contract.limn']])
(r/'report.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
print(json.dumps(report,indent=2,ensure_ascii=False))
