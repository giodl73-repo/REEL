"""Verify the complete packaged tree against its source and artifact records."""
import argparse,hashlib,json,pathlib,zipfile,subprocess
from git_source import read_git_blobs
from budget import RUNTIME_BUDGET_BYTES,SITE_BUDGET_BYTES,RUNTIME_FILES,runtime_bytes
parser=argparse.ArgumentParser();parser.add_argument('site',type=pathlib.Path);parser.add_argument('--commit',required=True);args=parser.parse_args()
site=args.site.resolve()
def sha(data):return hashlib.sha256(data).hexdigest()
def read(name):return json.loads((site/name).read_text(encoding='utf-8'))
inv=read('inventory.json');proof=read('provenance.json')
assert inv['commit']==proof['commit']==args.commit
assert {p.name for p in site.parent.iterdir()}=={'assembly','index.html'}
assert not (site.parent/'index.html').is_symlink()
assert sha((site.parent/'index.html').read_bytes())==proof['landing_index_sha256']
actual={p.relative_to(site).as_posix() for p in site.rglob('*') if p.is_file()}
assert not any(p.is_symlink() or any(part.startswith('.') for part in p.relative_to(site).parts) for p in site.rglob('*'))
records={r['path']:r for r in inv['files']};assert len(records)==len(inv['files'])
assert actual==set(records)|{'inventory.json'},(actual^set(records))
assert sum((site/n).stat().st_size for n in actual)+(site.parent/'index.html').stat().st_size<=SITE_BUDGET_BYTES
assert proof['budgets']=={'runtime_bytes':RUNTIME_BUDGET_BYTES,'complete_site_bytes':SITE_BUDGET_BYTES}
assert inv['runtime_bytes']==runtime_bytes(site)<=RUNTIME_BUDGET_BYTES
assert inv['runtime_files']==list(RUNTIME_FILES) and inv['runtime_includes_root_landing'] is True
for name,record in records.items():
 data=(site/name).read_bytes();assert len(data)==record['bytes'] and sha(data)==record['sha256'],name
assert sha((site/'source.zip').read_bytes())==proof['source_zip_sha256']
assert sha((site/'pkg/reel_assembly_workbench_bg.wasm').read_bytes())==proof['wasm_sha256']
with zipfile.ZipFile(site/'source.zip') as archive:
 pins=read('source-files.json');expected={'REEL-source/'+p['path']:p for p in pins}
 assert len(expected)==len(pins) and set(archive.namelist())==set(expected)
 git_paths=subprocess.check_output(['git','ls-tree','-r','--name-only',args.commit],cwd=pathlib.Path(__file__).resolve().parents[2]).decode().splitlines()
 assert set(git_paths)=={p['path'] for p in pins}
 blobs=read_git_blobs(pathlib.Path(__file__).resolve().parents[2],args.commit,git_paths)
 for name,record in expected.items():
  assert not pathlib.PurePosixPath(name).is_absolute() and '..' not in pathlib.PurePosixPath(name).parts
  data=archive.read(name)
  assert data==blobs[record['path']]
  assert sha(data)==record['sha256'] and len(data)==record['bytes']
packages=read('dependencies.json');assert len(packages)==proof['legal_packages']
for package in packages:
 assert package['notices']
 for notice in package['notices']:assert sha((site/notice['path']).read_bytes())==notice['sha256']
rust=read('rust/provenance.json');assert len(rust['notices'])==proof['rust_notice_files']
assert sha((site/'rust/COPYRIGHT-library.html').read_bytes())==rust['library_copyright_sha256']
for notice in rust['notices']:assert sha((site/notice['path']).read_bytes())==notice['sha256']
assert not any(n.endswith(('native-cases.json','cargo-metadata.json','browser-results.json','request.json')) for n in actual)
print(json.dumps({'commit':args.commit,'files':len(actual),'source_files':len(expected),'legal_packages':len(packages),'rust_notices':len(rust['notices']),'root_landing_verified':True,'verified':True}))
