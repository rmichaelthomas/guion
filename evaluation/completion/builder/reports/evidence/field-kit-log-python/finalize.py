from pathlib import Path
from datetime import datetime, timezone
import json
import shutil

root=Path('/private/tmp/veritas-fresh-builders/field-kit-log-python')
reports=Path('/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/evaluation/completion/builder/reports')
evidence=reports/'evidence'/'field-kit-log-python'
report_path=reports/'field-kit-log-python.json'
evidence.mkdir(parents=True,exist_ok=True)
# copy2 preserves original file timestamps; all artifacts are from this assignment.
files=[]
for original in sorted(root.rglob('*')):
    if not original.is_file() or '__pycache__' in original.parts:
        continue
    destination=evidence/original.relative_to(root)
    destination.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(original,destination)
    files.append(str(destination))
metrics=json.loads((root/'evidence'/'metrics.json').read_text())
times=json.loads((root/'evidence'/'phase-times.json').read_text())
def dt(value): return datetime.fromisoformat(value)
start=dt(times['start_utc'])
finish=datetime.now(timezone.utc)
setup=(dt(times['setup_end_utc'])-start).total_seconds()
build=(dt(times['initial_complete_utc'])-dt(times['setup_end_utc'])).total_seconds()
followup=(dt(times['followup_complete_utc'])-dt(times['followup_start_utc'])).total_seconds()
report={
 'schema_version':1,'assignment_id':'field-kit-log-python','status':'complete',
 'language':'python','brief_id':'field-kit-log','start_utc':start.isoformat(),
 'finish_utc':finish.isoformat(),'budget_minutes':15,
 'elapsed_seconds':(finish-start).total_seconds(),
 'initial_correct':True,'followup_correct':True,'relocation_passed':True,
 'setup_seconds':setup,'build_seconds':build,'followup_seconds':followup,
 'installed_launch_seconds':metrics['installed_launch_seconds'],
 'artifact_bytes':metrics['artifact_bytes'],
 'interventions':[
  'Builder-only acceptance fixture correction: first invocation selected data inside the source-runtime directory and was rejected by the intended storage guard. The app was copied to a dedicated runtime subdirectory and the preexisting caller-selected data directory remained separate; subsequent initial checks passed. No application rescue or human intervention.',
  'No network access, package installation, third-party dependencies, or other builder artifacts used.'
 ],
 'failures':[
  'Resolved initial fixture failure: first add exited 1 with "database must be outside the installed runtime directory"; retained in evidence/setup-path-rejection.json. No unresolved application or acceptance failures.'
 ],
 'recommendation':{
  'preferred_language':'python',
  'reason':'For this delivered maintainable local CLI, the pinned CPython standard library provided TOML parsing, transactional SQLite, local dates, argument validation, and a relocatable zipapp without dependency setup. This recommendation is based only on this implementation; no paired builder was inspected.',
  'observed_user_benefit':'An operator can add equipment and record pass/fail inspections without entering dates; records survive fresh processes, rejected inputs preserve database bytes, failed-only listing isolates repairs, and moving the installed app preserves access to a separate data directory.',
  'material_regressions':[]
 },
 'target_class':'maintainable-local-app',
 'evidence_paths':files+[str(report_path)],
 'runtime_version':'CPython 3.14.7',
 'timing_notes':{
  'start_basis':'First measured UTC clock at assignment start; setup includes skill/protocol reads, contract initialization, and runtime version confirmation. Contract file timestamp is the recorded setup/build boundary.',
  'phase_method':'UTC phase boundaries in evidence/phase-times.json; build includes initial implementation, fixture correction, and acceptance; followup includes the source edit and regression checks. Total elapsed additionally includes relocation, packaging, guide, evidence copy, and reporting.',
  'relocation_complete_utc':metrics['relocation_complete_utc'],
  'launch_definition':metrics['launch_definition'],
  'launch_samples_seconds':metrics['launch_samples_seconds']
 },
 'artifact_definition':metrics['artifact_definition'],
 'acceptance':{
  'item_creation':True,'pass_and_fail':True,'automatic_local_date':True,
  'separate_process_restart_persistence':True,'duplicate_preserves_db_bytes':True,
  'unknown_preserves_db_bytes':True,'invalid_preserves_db_bytes':True,
  'failed_only_filter':True,'unfiltered_behavior_after_edit':True,
  'new_writes_after_edit':True,'unicode_space_relocation':True,
  'preexisting_separate_data_directory':True,'second_runtime_move_same_data':True
 },
 'source_deliverable':str(evidence/'field_kit_log.py'),
 'installed_artifact':str(evidence/'runtime'/'field-kit-log.pyz'),
 'operator_guide':str(evidence/'OPERATOR.md'),
 'sample_toml':str(evidence/'config.sample.toml')
}
assert report['elapsed_seconds'] <= 900
assert all(Path(path).is_file() for path in files)
report_path.write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
# Check the saved report and every evidence reference, with no changes to assignments.
saved=json.loads(report_path.read_text())
assert all(Path(path).is_file() for path in saved['evidence_paths'])
assert saved['status']=='complete' and all(saved['acceptance'].values())
print(json.dumps({'report':str(report_path),'evidence_files':len(files),
 'elapsed_seconds':saved['elapsed_seconds'],'setup_seconds':setup,
 'build_seconds':build,'followup_seconds':followup,
 'finish_utc':saved['finish_utc']},indent=2))
