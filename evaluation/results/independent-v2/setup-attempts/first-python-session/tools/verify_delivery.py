"""Exercise only declared delivery artifacts with the source directory hidden."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
manifest = json.loads((ROOT / 'delivery.json').read_text())
with tempfile.TemporaryDirectory(prefix='sla-relocated-') as temporary:
    relocated = Path(temporary)
    for item in manifest['artifact_paths']:
        target = relocated / item
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / item, target)
    hidden = ROOT / 'work' / 'source-hidden-for-relocation'
    (ROOT / 'src').rename(hidden)
    try:
        env = os.environ.copy()
        env.pop('PYTHONPATH', None)
        env['SLA_REPORT_ARTIFACT'] = str(relocated / 'dist/sla-report')
        started = time.perf_counter()
        tests = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(ROOT / 'tests'), '-v'],
                               cwd=relocated, env=env, capture_output=True, text=True)
        tests_seconds = time.perf_counter() - started
        (ROOT / 'work' / 'tests-relocated.txt').write_text(tests.stdout + tests.stderr)
        if tests.returncode:
            print(tests.stdout + tests.stderr)
            raise SystemExit(tests.returncode)
        (relocated / 'tickets.csv').write_text('id,team,priority,status,created_at\n1,Support,high,open,2026-01-01T10:59:00Z\n')
        (relocated / 'policy.json').write_text('{"response_minutes":{"high":60}}')
        argv = [arg.replace('${candidate}', str(relocated)).replace('${workspace}', str(relocated))
                for arg in manifest['launch_command']]
        argv += ['--tickets', str(relocated / 'tickets.csv'), '--policy', str(relocated / 'policy.json'),
                 '--as-of', '2026-01-01T12:00:00Z', '--output', str(relocated / 'report.json')]
        started = time.perf_counter()
        run = subprocess.run(argv, cwd=relocated, env=env, capture_output=True, text=True)
        warm_seconds = time.perf_counter() - started
        assert run.returncode == 0, run.stderr
        result = json.loads((relocated / 'report.json').read_text())
        assert result['summary'] == {'total': 1, 'breached': 1}, result
        (ROOT / 'work' / 'sample-report.json').write_text((relocated / 'report.json').read_text())
        evidence = {
            'source_directory_hidden': True, 'tests_exit_code': tests.returncode,
            'tests_seconds': tests_seconds, 'runner_exit_code': run.returncode,
            'warm_run_seconds': warm_seconds,
            'relocated_contents_before_tests': manifest['artifact_paths'],
            'artifacts': {item: {'bytes': (relocated / item).stat().st_size,
                                 'sha256': hashlib.sha256((relocated / item).read_bytes()).hexdigest()}
                          for item in manifest['artifact_paths']}}
        (ROOT / 'work' / 'relocation-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
        print(json.dumps(evidence, indent=2))
    finally:
        hidden.rename(ROOT / 'src')
