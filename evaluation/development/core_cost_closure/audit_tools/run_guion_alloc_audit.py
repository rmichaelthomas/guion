#!/usr/bin/env python3
import hashlib
import json
import os
import random
import re
import subprocess
from pathlib import Path
binary=Path('/var/cache/guion-core-north-star/target/release/guion')
data=Path('/tmp/guion-core-cost-closure/sprint-20261029-current-baseline/jsonl-summary-data-200000')
artifact=data/'before.gub';summary=data/'performance-inputs/summary.json'
expected='b1b0afe4074a5a945c2ae6a097cb269174999dd6545ef380f5008ba8c36dd7fd'
rng=random.Random(20261031);records=[]
for pair in range(5):
 modes=['off','on'];rng.shuffle(modes)
 for mode in modes:
  env=os.environ.copy();env['GUION_PHASE_TIMINGS']='1';env.pop('GUION_RUNTIME_PROFILE',None);env.pop('GUION_COST_LEDGER',None)
  if mode=='on':env['LD_PRELOAD']='/tmp/guion_alloc_counter.so'
  else:env.pop('LD_PRELOAD',None)
  p=subprocess.run([str(binary),'run','--json',str(artifact),'--events','performance-inputs/events.jsonl','--output','performance-inputs/summary.json'],cwd=data,env=env,capture_output=True,text=True,check=True)
  report=json.loads(p.stdout);assert report['status']=='passed';assert hashlib.sha256(summary.read_bytes()).hexdigest()==expected
  found=re.findall(r'GUION_ALLOC (\{[^\n]+\})',p.stderr);assert len(found)==(1 if mode=='on' else 0),p.stderr
  alloc=json.loads(found[0]) if found else None
  elapsed=report['phase_timings_ns']['program_execute_ns'];records.append({'pair':pair,'mode':mode,'execute_ns':elapsed,'allocation_api':alloc})
  print(pair,mode,round(elapsed/1e9,6),flush=True)
Path('/tmp/guion-alloc-audit-runs.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'artifact_sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'output_sha256':expected,'records':records},indent=2)+'\n')
