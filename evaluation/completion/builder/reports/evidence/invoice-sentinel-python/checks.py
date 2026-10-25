"""Independent acceptance checks. Every child command and output is preserved."""
from pathlib import Path
import json
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
PHASE = sys.argv[1]
COPY = ROOT / 'relocation' / 'Factures Café 🧾'
CALLER = ROOT / 'different working directory'
COPY.mkdir(parents=True, exist_ok=True)
CALLER.mkdir(exist_ok=True)
shutil.copy2(ROOT/'invoice_sentinel.py', COPY/'invoice_sentinel.py')
shutil.copytree(ROOT/'fixtures', COPY/'fixtures', dirs_exist_ok=True)
OUT = CALLER / f'{PHASE} report.json'
LOG = ROOT / f'{PHASE}-commands.jsonl'
RESULTS = []
DURATIONS = []

def run(extra=(), input_name='ledger.tsv', as_of='2026-02-01', out=OUT):
    command = [sys.executable, str(COPY/'invoice_sentinel.py'), '--input', str(COPY/'fixtures'/input_name), '--as-of', as_of, '--output', str(out), *extra]
    start = time.perf_counter()
    proc = subprocess.run(command, cwd=CALLER, capture_output=True, text=True)
    elapsed = time.perf_counter()-start
    DURATIONS.append(elapsed)
    with LOG.open('a', encoding='utf-8') as log:
        log.write(json.dumps({'argv': command, 'cwd': str(CALLER), 'seconds': elapsed, 'returncode': proc.returncode, 'stdout': proc.stdout, 'stderr': proc.stderr}, ensure_ascii=False)+'\n')
    return proc

def check(name, condition):
    RESULTS.append({'name': name, 'passed': bool(condition)})
    if not condition:
        print('FAIL:', name)

proc = run()
check('success with malformed rows', proc.returncode == 0)
result = json.loads(OUT.read_text())
expected = [
    {'vendor': 'Acme', 'overdue_open_count': 1, 'overdue_open_cents': 1200, 'total_open_cents': 1500},
    {'vendor': 'Beta', 'overdue_open_count': 0, 'overdue_open_cents': 0, 'total_open_cents': 500},
    {'vendor': 'Café Supplies', 'overdue_open_count': 0, 'overdue_open_cents': 0, 'total_open_cents': 900},
]
check('multiple vendors, paid/open, overdue/future, due today', result['vendors'] == expected)
check('four rejected rows with physical line numbers and reasons', result['rejected_row_count'] == 4 and [x['line'] for x in result['rejected_rows']] == [7,8,9,10] and all(x['reason'] for x in result['rejected_rows']))
check('each rejection reported on stderr', all(f'line {n}:' in proc.stderr for n in [7,8,9,10]))
check('no excluded field without filter', 'excluded_row_count' not in result)
baseline = OUT.read_bytes()
run()
check('deterministic repeat output', OUT.read_bytes() == baseline)
for bad_date in ['2026-02-30', '20260201', '2026-2-01', 'nonsense']:
    proc = run(as_of=bad_date)
    check(f'invalid as-of preserves existing output: {bad_date}', proc.returncode != 0 and OUT.read_bytes() == baseline)
proc = run(input_name='bad-header.tsv')
check('bad header preserves existing output', proc.returncode != 0 and OUT.read_bytes() == baseline)
proc = run(input_name='missing.tsv')
check('missing input preserves existing output', proc.returncode != 0 and OUT.read_bytes() == baseline)
proc = run(input_name='empty.tsv', out=CALLER/f'{PHASE}-empty.json')
empty = json.loads((CALLER/f'{PHASE}-empty.json').read_text())
check('header-only ledger', proc.returncode == 0 and empty['vendors'] == [] and empty['rejected_row_count'] == 0)
# Very large integer and negative credit establish exact integer arithmetic.
(COPY/'fixtures'/'integer.tsv').write_text('invoice_id\tvendor\tamount_cents\tdue_date\tstate\nx\tAcme\t900719925474099312345\t2026-01-01\topen\ny\tAcme\t-2\t2026-01-01\topen\nz\tAcme\t1e2\t2026-01-01\topen\n')
proc = run(input_name='integer.tsv', out=CALLER/f'{PHASE}-integer.json')
integer = json.loads((CALLER/f'{PHASE}-integer.json').read_text())
check('exact large integer arithmetic and rejection of scientific notation', proc.returncode == 0 and integer['vendors'][0]['total_open_cents'] == 900719925474099312343 and integer['rejected_row_count'] == 1)
shutil.copy2(COPY/'fixtures'/'integer.tsv', ROOT/'fixtures'/'integer.tsv')
if PHASE == 'followup':
    initial = json.loads((ROOT/'initial-results.json').read_text())
    check('no-option bytes identical to initial version', baseline == (ROOT/'initial-output.json').read_bytes())
    proc = run(['--vendor', 'Acme'])
    selected = json.loads(OUT.read_text())
    check('exact vendor filter', proc.returncode == 0 and selected['vendors'] == expected[:1] and selected['excluded_row_count'] == 3 and selected['rejected_row_count'] == 4)
    selected_bytes = OUT.read_bytes()
    run(['--vendor', 'Acme'])
    check('filtered repeat deterministic', OUT.read_bytes() == selected_bytes)
    run(['--vendor', 'acme'])
    selected = json.loads(OUT.read_text())
    check('case-sensitive absent vendor', selected['vendors'] == [] and selected['excluded_row_count'] == 5)
    run(['--vendor', 'Café Supplies'])
    selected = json.loads(OUT.read_text())
    check('Unicode vendor with spaces', selected['vendors'] == expected[2:] and selected['excluded_row_count'] == 4)
    run(['--vendor', ''])
    selected = json.loads(OUT.read_text())
    check('empty exact filter excludes every valid row', selected['vendors'] == [] and selected['excluded_row_count'] == 5)
    before = OUT.read_bytes()
    proc = run(['--vendor', 'Acme'], as_of='2026-13-01')
    check('invalid date with filter preserves output', proc.returncode != 0 and OUT.read_bytes() == before)
else:
    (ROOT/'initial-output.json').write_bytes(baseline)
summary = {'phase': PHASE, 'checks': RESULTS, 'passed': all(x['passed'] for x in RESULTS), 'launch_seconds': DURATIONS, 'relocation_path': str(COPY), 'caller_cwd': str(CALLER)}
(ROOT/f'{PHASE}-results.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2)+'\n')
print(json.dumps({'phase': PHASE, 'passed': summary['passed'], 'checks': len(RESULTS), 'launch_min_seconds': min(DURATIONS), 'launch_max_seconds': max(DURATIONS)}))
raise SystemExit(0 if summary['passed'] else 1)
