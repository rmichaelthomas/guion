from pathlib import Path
from datetime import datetime, timezone
import json
root = Path(__file__).parent
original = json.loads((root/'report.json').read_text())
(root/'detailed-report.json').write_text(json.dumps(original, ensure_ascii=False, indent=2)+'\n')
report = {
 'schema_version':1,
 'status':'completed',
 'assignment_id':original['assignment_id'],
 'language':original['language'],
 'brief_id':original['brief_id'],
 'start_utc':original['start_utc'],
 'finish_utc':datetime.now(timezone.utc).isoformat(),
 'initial_correct':original['correctness']['initial']['passed'],
 'followup_correct':original['correctness']['followup']['passed'],
 'relocation_passed':original['relocation_result']['passed'],
 'setup_seconds':original['self_reported_times_seconds']['setup'],
 'build_seconds':original['self_reported_times_seconds']['initial_build_and_checks'],
 'followup_seconds':(datetime.now(timezone.utc)-datetime.fromisoformat(json.loads((root/'followup-start.json').read_text())['utc'])).total_seconds(),
 'installed_launch_seconds':original['launch_duration_seconds']['median'],
 'artifact_bytes':sum(p.stat().st_size for p in root.rglob('*') if p.is_file() and p.name!='report.json'),
 'interventions': original['interventions'] + ['Normalized the final report to the parent-requested schema; detailed evidence retained in detailed-report.json.'],
 'failures':original['failures'],
 'recommendation': {
   'preferred_language':'python',
   'reason':'Python 3.14.7 standard library completed this brief with no installation, 14 initial checks and 21 follow-up checks passing. I observed no other language implementation, so this is a task-specific recommendation without a comparative ranking.',
   'observed_user_benefit':'No dependencies or build step; approximately 54 ms median launch; byte-stable original behavior after the filter addition; working copies launched from another directory at a Unicode-and-spaces path.',
   'material_regressions':[]
 }
}
(root/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(report, ensure_ascii=False, indent=2))
