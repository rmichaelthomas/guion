from datetime import datetime, timezone
from pathlib import Path
import json
import statistics
root = Path(__file__).parent
started = json.loads((root/'started.json').read_text())
followup_start = json.loads((root/'followup-start.json').read_text())
initial = json.loads((root/'initial-results.json').read_text())
followup = json.loads((root/'followup-results.json').read_text())
finish = datetime.now(timezone.utc)
start = datetime.fromisoformat(started['start_utc'])
followup_at = datetime.fromisoformat(followup_start['utc'])
files = [p for p in root.rglob('*') if p.is_file() and p.name != 'report.json']
launch = initial['launch_seconds'] + followup['launch_seconds']
report = {
 'assignment_id': 'invoice-sentinel-python', 'language': 'python', 'brief_id': 'invoice-sentinel',
 'start_utc': started['start_utc'], 'finish_utc': finish.isoformat(),
 'runtime': started['python'],
 'self_reported_times_seconds': {
    'setup': started['setup_seconds'],
    'initial_build_and_checks': (followup_at-start).total_seconds(),
    'followup_checks_and_documentation': (finish-followup_at).total_seconds(),
    'total_measured_after_setup': (finish-start).total_seconds(),
    'note': 'Setup is an approximately 30-second estimate for skill/contract opening and runtime confirmation before the recorded build start. Build and follow-up include authoring, checks, and evidence preparation.'
 },
 'correctness': {
   'initial': {'passed': initial['passed'], 'checks_passed': sum(c['passed'] for c in initial['checks']), 'checks_total':len(initial['checks']), 'evidence':'initial-results.json'},
   'followup': {'passed': followup['passed'], 'checks_passed': sum(c['passed'] for c in followup['checks']), 'checks_total':len(followup['checks']), 'evidence':'followup-results.json'},
   'scope': 'Self-authored acceptance checks from the supplied brief; not an independent external grading claim.'
 },
 'relocation_result': {'passed': initial['passed'] and followup['passed'], 'source_path': str(root), 'copied_project_path': followup['relocation_path'], 'launch_cwd':followup['caller_cwd'], 'both_phases_tested':True},
 'launch_duration_seconds': {'first': launch[0], 'minimum':min(launch), 'median':statistics.median(launch), 'maximum':max(launch), 'sample_count':len(launch), 'definition':'End-to-end Python process launch, file read, validation, aggregation, output write, and process exit; includes expected argument-error runs.'},
 'artifact_footprint': {'files_excluding_report':len(files), 'logical_bytes_excluding_report':sum(p.stat().st_size for p in files), 'final_source_bytes': (root/'invoice_sentinel.py').stat().st_size, 'third_party_dependencies':0, 'files':[{'path':str(p.relative_to(root)), 'bytes':p.stat().st_size} for p in sorted(files)]},
 'interventions': [
    'Authored initial CLI and acceptance harness directly from the supplied brief.',
    'Preserved the initial source and applied a local source patch for the requested vendor filter.',
    'No network, package installation, repository inspection, prior builder output, or external code assistance used.',
    'No failed acceptance check required corrective intervention.'
 ],
 'failures': [],
 'interpretations': [
    'Dates or amounts malformed inside ledger rows are rejected and reported before atomic replacement; valid rows still produce a new report. Invalid CLI as-of dates are fatal and preserve the prior destination.',
    'Signed integer amounts accepted because the brief did not prohibit credits.',
    'Dates equal to as-of are not overdue.',
    'Filter exclusion counts all otherwise-valid rows from other vendors, including paid rows.'
 ],
 'preference_recommendation': 'I recommend this Python standard-library implementation for the supplied Invoice Sentinel task: the observed Python 3.14.7 environment required no installation, all 35 phase checks passed, the follow-up preserved original output bytes, relocation worked, and launches completed in roughly 50–62 milliseconds. I did not observe any alternative language implementation and make no comparative ranking.'
}
(root/'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False)+'\n')
with (root/'session-contract.limn').open('a') as stream:
    stream.write('remember a string called source-state with "verified"\nremember a string called claim-basis with "verified"\nadd "Initial 14 checks and follow-up 21 checks passed from a relocated Unicode-and-spaces path" to tracked-decisions\n')
print(json.dumps({'report':str(root/'report.json'),'total_seconds':report['self_reported_times_seconds']['total_measured_after_setup'],'initial_checks':len(initial['checks']),'followup_checks':len(followup['checks']),'footprint_bytes':report['artifact_footprint']['logical_bytes_excluding_report']}))
