import json, os, shutil, subprocess, tomllib
from pathlib import Path
root=Path('/src');work=root/'work';scratch=work/'config-migrator-linux-pilot'
if scratch.exists(): shutil.rmtree(scratch)
scratch.mkdir(parents=True)
project=scratch/'source';package=scratch/'package';moved=scratch/'moved'
linux=root/'distribution/out-north-star-sprint-current-linux/veritas-0.1.0-linux-aarch64'
cache=scratch/'uv-cache';python_install=scratch/'python-install'
shutil.copytree(linux/'examples/package_analysis_bundle/.uv-cache',cache,symlinks=True)
shutil.copytree(work/'veritas-configpilot-uv-cache',cache,dirs_exist_ok=True,symlinks=True)
shutil.copytree(linux/'examples/package_analysis_bundle/python',python_install,symlinks=True)
shutil.copytree(root/'evaluation/pilot/author-self-review/config_migrator',project,ignore=shutil.ignore_patterns('.veritas','veritas.lock','__pycache__'))
binary=linux/'veritas';uv=linux/'toolchains/uv'
env=os.environ.copy();env.update(VERITAS_UV=str(uv),UV_CACHE_DIR=str(cache),UV_PYTHON_INSTALL_DIR=str(python_install),VERITAS_PACKAGE_UV_CACHE=str(cache))
def run(cmd,cwd=None):
 r=subprocess.run([str(x) for x in cmd],cwd=cwd,env=env,text=True,capture_output=True)
 if r.returncode: raise RuntimeError(f"failed ({r.returncode}): {' '.join(map(str,cmd))}\n{r.stdout}\n{r.stderr}")
 return r
print('syncing locked Linux ARM64 Python dependency',flush=True)
run([binary,'sync','--update',project])
print('packaging the TOML migrator for Linux ARM64',flush=True)
run([binary,'package',project,'--output',package]);run([binary,'verify-package',package])
shutil.copytree(package,moved,symlinks=True)
config=moved/'config.toml';config.write_text('# owner comment\nversion = 2\nname = "demo"\n[sync]\ntimeout_seconds = 45\n',encoding='utf-8')
print('testing relocated, source-free Linux app',flush=True)
run([moved/'run','migrate','--file','config.toml'],cwd=moved);run([moved/'run','check','--file','config.toml'],cwd=moved);run([binary,'verify-package',moved])
data=tomllib.loads(config.read_text(encoding='utf-8'));assert data=={'version':3,'name':'demo','sync':{'timeout_seconds':45,'parallelism':4}},data
assert '# owner comment' in config.read_text(encoding='utf-8') and not (moved/'main.vr').exists()
report={'status':'author-run-package-acceptance','target':'linux-aarch64','package_directory_bytes':sum(p.stat().st_size for p in package.rglob('*') if p.is_file()),'source_free_after_relocation':True,'migration_result':data,'verification_after_launch':'passed'}
(scratch/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2),flush=True)
