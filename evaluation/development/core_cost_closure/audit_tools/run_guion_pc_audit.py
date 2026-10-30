#!/usr/bin/env python3
import hashlib
import json
import os
import random
import subprocess
from pathlib import Path
binary=Path('/var/cache/guion-core-north-star/target/release/guion')
data=Path('/tmp/guion-core-cost-closure/sprint-20261029-current-baseline/jsonl-summary-data-200000')
artifact=data/'before.gub'
summary=data/'performance-inputs/summary.json'
expected='b1b0afe4074a5a945c2ae6a097cb269174999dd6545ef380f5008ba8c36dd7fd'
order=[];rng=random.Random(20261030)
for pair in range(10):
 modes=['off','on'];rng.shuffle(modes);order.extend((pair,mode) for mode in modes)
records=[]
for pair,mode in order:
 env=os.environ.copy();env['GUION_PHASE_TIMINGS']='1';env.pop('GUION_RUNTIME_PROFILE',None);env.pop('GUION_COST_LEDGER',None)
 sample_path=f'/tmp/guion-pc-debug-pair-{pair:02d}.txt'
 if mode=='on':env['LD_PRELOAD']='/tmp/guion_pc_sampler.so';env['GUION_PC_SAMPLE_PATH']=sample_path
 else:env.pop('LD_PRELOAD',None);env.pop('GUION_PC_SAMPLE_PATH',None)
 p=subprocess.run([str(binary),'run','--json',str(artifact),'--events','performance-inputs/events.jsonl','--output','performance-inputs/summary.json'],cwd=data,env=env,capture_output=True,text=True,check=True)
 report=json.loads(p.stdout);assert report['status']=='passed';digest=hashlib.sha256(summary.read_bytes()).hexdigest();assert digest==expected,digest
 elapsed=report['phase_timings_ns']['program_execute_ns'];records.append({'pair':pair,'mode':mode,'execute_ns':elapsed,'sample_path':sample_path if mode=='on' else None})
 print(pair,mode,round(elapsed/1e9,6),flush=True)
Path('/tmp/guion-pc-audit-runs.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'artifact_sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'output_sha256':expected,'records':records},indent=2)+'\n')
