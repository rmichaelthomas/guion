import pathlib, subprocess, time, json, datetime, shutil
r=pathlib.Path(__file__).parent
v='/Users/rmichaelthomas/Documents/Codex/2026-10-22/referenced-chatgpt-conversation-this-is-an/work/veritas-foundation/target/completion/releases/macos-aarch64/current/bin/veritas'
records=[]
def timed(cmd,cwd=None):
    start=datetime.datetime.now(datetime.timezone.utc).isoformat(); t=time.perf_counter();p=subprocess.run(cmd,cwd=cwd,capture_output=True,text=True)
    record=dict(command=cmd,cwd=str(cwd or pathlib.Path.cwd()),start_utc=start,seconds=time.perf_counter()-t,returncode=p.returncode,stdout=p.stdout,stderr=p.stderr)
    records.append(record)
    if p.returncode: raise RuntimeError(record)
    return record
build=timed([v,'build','--json',str(r/'project/main.vr'),'--output',str(r/'invoice-sentinel.vrb')])
verification=timed([v,'verify-package','--json',str(r/'final-app')])
launch=timed([str(r/'installed/bin/invoice-sentinel'),'--input','ledger.tsv','--as-of','2026-01-15','--output','installed-report.json','--vendor','Acme'],r/'checks-followup')
assert json.loads((r/'checks-followup/installed-report.json').read_text())['vendors']['Acme']['open_cents']==450
sizes={'artifact_bytes':(r/'invoice-sentinel.vrb').stat().st_size,'initial_source_bytes':(r/'initial.vr').stat().st_size,'final_source_bytes':(r/'project/main.vr').stat().st_size,'package_bytes':sum(p.stat().st_size for p in (r/'final-app').rglob('*') if p.is_file())}
(r/'measurements.json').write_text(json.dumps(dict(commands=records,sizes=sizes),indent=2))
print(json.dumps(dict(build_seconds=build['seconds'],installed_launch_seconds=launch['seconds'],sizes=sizes)))
for p in [r/'project/veritas.toml',r/'initial.vr',r/'project/main.vr',r/'commands-initial.json',r/'commands-followup.json']:
 print(p.name,datetime.datetime.fromtimestamp(p.stat().st_mtime,datetime.timezone.utc).isoformat())
