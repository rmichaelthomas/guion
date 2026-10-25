from pathlib import Path
from datetime import datetime, timezone, date
import json
import shutil
import statistics
import subprocess
import sys
import time
import zipapp
import verify

root = Path(__file__).resolve().parent
try:
    before = (root / 'evidence' / 'initial-list.tsv').read_text()
    assert verify.call('list') == before
    digest = verify.snapshot()
    failed = verify.call('list', '--failed-only')
    assert failed.splitlines() == [before.splitlines()[0], before.splitlines()[2]]
    assert verify.snapshot() == digest
    # Regression: valid writes, old unfiltered listing, persistence, invalid inputs.
    verify.call('add', 'meter-3', 'Test meter')
    verify.call('inspect', 'meter-3', 'pass', 'Reading stable')
    verify.call('inspect', 'meter-3', 'fail', 'Probe loose')
    full = verify.call('list')
    assert len(full.splitlines()) == 5
    assert full.splitlines()[:3] == before.splitlines()
    assert all(r.split('\t')[2] == date.today().isoformat() for r in full.splitlines()[1:])
    assert verify.call('list') == full
    assert len(verify.call('list', '--failed-only').splitlines()) == 3
    for command in [('add', 'radio-1', 'Duplicate'), ('inspect', 'unknown', 'pass', 'Missing'),
                    ('inspect', 'radio-1', 'invalid', 'Invalid')]:
        digest = verify.snapshot()
        verify.call(*command, expected=1)
        assert verify.snapshot() == digest
        assert verify.call('list') == full
    times = json.loads((root / 'evidence' / 'phase-times.json').read_text())
    times['followup_complete_utc'] = datetime.now(timezone.utc).isoformat()
    (root / 'evidence' / 'phase-times.json').write_text(json.dumps(times, indent=2))
    # Package the installed artifact; the interpreter is the pinned external runtime.
    stage = root / 'package-stage'
    stage.mkdir(exist_ok=True)
    shutil.copy2(root / 'field_kit_log.py', stage / '__main__.py')
    installed = root / 'runtime' / 'field-kit-log.pyz'
    zipapp.create_archive(stage, installed, interpreter='/usr/bin/env python3')
    (root / 'runtime' / 'field_kit_log.py').unlink()
    # Caller creates an existing, separate data directory before installing app.
    relocation = root / 'Relocated kit café Δ'
    relocated_runtime = relocation / 'installed app'
    selected_data = relocation / 'Team data Zürich'
    selected_data.mkdir(parents=True, exist_ok=True)
    data_created = selected_data.stat().st_mtime
    relocated_runtime.mkdir()
    shutil.copy2(installed, relocated_runtime / installed.name)
    config = relocation / 'operator config.toml'
    config.write_text('[storage]\ndatabase = ' + json.dumps(str(selected_data / 'field.sqlite3'), ensure_ascii=False) + '\n')
    app = relocated_runtime / installed.name
    verify.call('add', 'équipement-1', 'Radio Δ', app=app, config=config)
    verify.call('inspect', 'équipement-1', 'pass', 'Chargée', app=app, config=config)
    verify.call('inspect', 'équipement-1', 'fail', 'Antenne cassée', app=app, config=config)
    history = verify.call('list', app=app, config=config)
    assert len(history.splitlines()) == 3
    assert verify.call('list', app=app, config=config) == history
    assert len(verify.call('list', '--failed-only', app=app, config=config).splitlines()) == 2
    assert all(row.split('\t')[2] == date.today().isoformat() for row in history.splitlines()[1:])
    db = selected_data / 'field.sqlite3'
    assert db.is_file() and not list(relocated_runtime.glob('*.sqlite3'))
    assert data_created <= app.stat().st_ctime
    # Also move the installed app while retaining the same selected database.
    moved_runtime = relocation / 'Moved runtime Ω'
    shutil.move(str(relocated_runtime), moved_runtime)
    app = moved_runtime / installed.name
    assert verify.call('list', app=app, config=config) == history
    launch_times = []
    for _ in range(5):
        t = time.perf_counter()
        result = subprocess.run([sys.executable, str(app), '--help'], capture_output=True, text=True)
        launch_times.append(time.perf_counter() - t)
        assert result.returncode == 0 and 'Field Kit Log' in result.stdout
    metrics = {'initial_correct':True,'followup_correct':True,'relocation_passed':True,
               'installed_launch_seconds':statistics.median(launch_times),
               'launch_samples_seconds':launch_times,'launch_definition':'median of five fresh Python processes invoking installed zipapp --help',
               'artifact_bytes':installed.stat().st_size,'data_directory_preexisted_install':True,
               'relocated_runtime':str(app),'separate_data_directory':str(selected_data),
               'relocation_complete_utc':datetime.now(timezone.utc).isoformat()}
    (root / 'evidence' / 'metrics.json').write_text(json.dumps(metrics,indent=2))
    (root / 'evidence' / 'followup-list.tsv').write_text(full)
    (root / 'evidence' / 'failed-only.tsv').write_text(failed)
    (root / 'evidence' / 'relocated-list.tsv').write_text(history)
    print(json.dumps(metrics, indent=2))
    print('FOLLOW-UP AND RELOCATION ACCEPTANCE PASSED')
finally:
    (root / 'evidence' / 'followup-acceptance.json').write_text(json.dumps(verify.LOG,indent=2,ensure_ascii=False))
