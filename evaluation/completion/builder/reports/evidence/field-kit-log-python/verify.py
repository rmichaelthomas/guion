"""Acceptance driver; only the standard library is used."""
from pathlib import Path
from datetime import datetime, timezone, date
import hashlib
import json
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
DATA = ROOT / 'operator data'
DATA.mkdir(exist_ok=True)
CONFIG = ROOT / 'acceptance.toml'
CONFIG.write_text('[storage]\ndatabase = ' + json.dumps(str(DATA / 'kit.sqlite3')) + '\n')
LOG = []

def call(*args, expected=0, app=None, config=None):
    command = [sys.executable, str(app or ROOT / 'runtime' / 'field_kit_log.py'), '--config', str(config or CONFIG), *args]
    start = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True)
    LOG.append({'command': command, 'exit_code': result.returncode, 'stdout': result.stdout,
                'stderr': result.stderr, 'seconds': time.perf_counter() - start,
                'utc': datetime.now(timezone.utc).isoformat()})
    assert (result.returncode == 0) == (expected == 0), LOG[-1]
    return result.stdout


def snapshot():
    return hashlib.sha256((DATA / 'kit.sqlite3').read_bytes()).hexdigest()


def initial():
    call('add', 'radio-1', 'Field radio')
    call('add', 'lamp-2', 'Portable lamp')
    call('inspect', 'radio-1', 'pass', 'Battery charged')
    call('inspect', 'lamp-2', 'fail', 'Lens cracked')
    before = call('list')
    rows = before.splitlines()
    assert len(rows) == 3 and rows[1].startswith('1\tradio-1\t') and rows[2].startswith('2\tlamp-2\t')
    assert all(row.split('\t')[2] == date.today().isoformat() for row in rows[1:])
    assert call('list') == before  # Each invocation is a newly started process.
    for command in [('add', 'radio-1', 'Duplicate'), ('inspect', 'unknown', 'pass', 'Missing'),
                    ('inspect', 'radio-1', 'maybe', 'Invalid'), ('add', '', 'Blank ID'),
                    ('inspect', 'radio-1', 'pass', '')]:
        digest = snapshot()
        call(*command, expected=1)
        assert snapshot() == digest
        assert call('list') == before
    (ROOT / 'evidence' / 'initial-list.tsv').write_text(before)
    return before

if __name__ == '__main__':
    try:
        initial()
        print('INITIAL ACCEPTANCE PASSED')
    finally:
        (ROOT / 'evidence' / 'initial-acceptance.json').write_text(json.dumps(LOG, indent=2))
