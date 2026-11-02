"""Stage a clean-source release candidate. Does not deploy."""
import hashlib, html, json, os, pathlib, shutil, subprocess, zipfile
from git_source import read_git_blobs
P=pathlib.Path(__file__).resolve().parent
ROOT=P.parent.parent
SITE=ROOT/'target/reel-release/REEL/assembly'
PROTOTYPE=ROOT/'target/reel-prototype/REEL/assembly'

def run(*args):
    return subprocess.check_output(args,cwd=ROOT)
def digest(data): return hashlib.sha256(data).hexdigest()
def write_json(path,value): path.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8')
def copy(source,destination):
    assert not source.is_symlink(),source
    destination.parent.mkdir(parents=True,exist_ok=True)
    destination.write_bytes(source.read_bytes())

if run('git','status','--porcelain').strip():
    raise SystemExit('Release staging requires a clean checkout. Commit source first.')
head=run('git','rev-parse','HEAD').decode().strip()
run('git','diff','--exit-code','e4d22e0d2ee4e447dcee3f3b1ae21c7fac91462b','--','crates/reel-assembly')
assert SITE.resolve()==(ROOT.resolve()/'target/reel-release/REEL/assembly')
if SITE.exists(): shutil.rmtree(SITE)
SITE.mkdir(parents=True)
landing=run('git','show',head+':browser/assembly/web/landing.html')
(SITE.parent/'index.html').write_bytes(landing)
for name in ['index.html','style.css','app.js','worker.js','graph.json']:
    source=PROTOTYPE/name
    expected=ROOT/('browser/assembly/fixtures/graph.json' if name=='graph.json' else 'browser/assembly/web/'+name)
    assert source.read_bytes()==expected.read_bytes()==run('git','show',head+':'+expected.relative_to(ROOT).as_posix()),f'Stale/non-Git prototype source: {name}'
    copy(source,SITE/name)
for name in ['reel_assembly_workbench.js','reel_assembly_workbench_bg.wasm']:
    copy(PROTOTYPE/'pkg'/name,SITE/'pkg'/name)
copy(ROOT/'LICENSE',SITE/'LICENSE')
paths=run('git','ls-files','-z').decode().split('\0')
blobs=read_git_blobs(ROOT,head,filter(None,paths))
source_pins=[]
with zipfile.ZipFile(SITE/'source.zip','w',zipfile.ZIP_DEFLATED,compresslevel=9) as archive:
    for name in sorted(filter(None,paths)):
        data=blobs[name]
        # The exact Git source is independent of checkout line-ending settings.
        archive.writestr('REEL-source/'+name,data)
        source_pins.append({'path':name,'sha256':digest(data),'bytes':len(data)})
(SITE/'source-files.json').write_text(json.dumps(source_pins,separators=(',',':'))+'\n',encoding='utf-8')
metadata=json.loads(run('cargo','+1.95.0','metadata','--manifest-path','browser/assembly/Cargo.toml','--locked','--format-version','1','--filter-platform','wasm32-unknown-unknown'))
assert all(p.get('source') is None for p in metadata['packages'] if p['name'] in ['reel-assembly','reel-assembly-workbench'])
packages=[]
for package in metadata['packages']:
    folder=pathlib.Path(package['manifest_path']).parent
    notice_root=folder
    if package['name'] in ['reel-assembly','reel-assembly-workbench']: notice_root=ROOT
    elif package.get('source','') and package['source'].startswith('git+'):
        for ancestor in [folder,*folder.parents[:4]]:
            if (ancestor/'LICENSE').is_file(): notice_root=ancestor;break
    notices=[f for f in notice_root.iterdir() if f.is_file() and f.name.upper().startswith(('LICENSE','COPYRIGHT','NOTICE','UNLICENSE'))]
    if not notices: raise SystemExit('No license notices for '+package['name'])
    records=[]
    for notice in sorted(notices):
        destination=SITE/'licenses/dependencies'/(package['name']+'-'+package['version'])/notice.name
        copy(notice,destination)
        records.append({'path':destination.relative_to(SITE).as_posix(),'sha256':digest(notice.read_bytes())})
    license_expression=package['license']
    packages.append({'name':package['name'],'version':package['version'],'source':package.get('source') or 'first-party REEL source.zip','license':license_expression,'notices':records})
write_json(SITE/'dependencies.json',packages)
sysroot=pathlib.Path(run('rustc','+1.95.0','--print','sysroot').decode().strip())
rust_docs=sysroot/'share/doc/rust'
copy(rust_docs/'COPYRIGHT-library.html',SITE/'rust/COPYRIGHT-library.html')
rust_notices=[]
for notice in sorted((rust_docs/'licenses').rglob('*')):
    if notice.is_file():
        destination=SITE/'rust/licenses'/notice.relative_to(rust_docs/'licenses')
        copy(notice,destination)
        rust_notices.append({'path':destination.relative_to(SITE).as_posix(),'sha256':digest(notice.read_bytes())})
write_json(SITE/'rust/provenance.json',{'rustc':run('rustc','+1.95.0','-vV').decode().strip(),'library_copyright_sha256':digest((SITE/'rust/COPYRIGHT-library.html').read_bytes()),'notices':rust_notices})
links=['<h1>REEL software and dependency licenses</h1><p>Original REEL software and ordinary software documentation are MIT; original non-software content retains CC BY-NC 4.0. Vendor and dependency grants retain their own scope. Third-party terms and prior grants remain intact.</p><p><a href="../LICENSE">REEL complete license</a> | <a href="../dependencies.json">Dependency inventory</a> | <a href="../rust/COPYRIGHT-library.html">Rust library copyright</a> | <a href="../rust/provenance.json">Rust notices and provenance</a></p>']
for package in packages:
    links.append('<h2>'+html.escape(package['name']+' '+package['version'])+'</h2><p>'+html.escape(package['license'] or 'See original notices')+'</p>')
    for notice in package['notices']:
        path=notice['path'].removeprefix('licenses/')
        links.append('<p><a href="'+html.escape(path)+'">'+html.escape(path)+'</a></p>')
(SITE/'licenses/index.html').write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>REEL licenses</title><body>'+''.join(links)+'</body></html>',encoding='utf-8')
copy(P/'BUILD.md',SITE/'BUILD.md')
provenance={'schema':'reel-assembly-workbench-release-v1','commit':head,'foundation_source':'e4d22e0d2ee4e447dcee3f3b1ae21c7fac91462b','foundation_scope':'crates/reel-assembly','initial_production_integration':'0e34cc35f184dd7d68d47c328843c105f349bd97','repository':'https://github.com/giodl73-repo/REEL','exact_full_source':'https://github.com/giodl73-repo/REEL/archive/'+head+'.zip','rustc':run('rustc','+1.95.0','-vV').decode().strip(),'wasm_bindgen':'0.2.127','wasm_debug_name_section':'removed with wasm-bindgen --remove-name-section','source_zip_sha256':digest((SITE/'source.zip').read_bytes()),'wasm_sha256':digest((SITE/'pkg/reel_assembly_workbench_bg.wasm').read_bytes()),'legal_packages':len(packages),'rust_notice_files':len(rust_notices),'landing_index_sha256':digest(landing),'cross_directory_binary_identity':'Not established. Verify native/WASM response parity instead.','scope':'Existing REEL semantic dependency diagnostics. Local input-bearing reports are not privacy-safe release receipts; no media, cache bytes, rights, creative selection or production release is verified.'}
write_json(SITE/'provenance.json',provenance)
files=[]
for f in sorted(SITE.rglob('*')):
    if f.is_file(): files.append({'path':f.relative_to(SITE).as_posix(),'bytes':f.stat().st_size,'sha256':digest(f.read_bytes())})
inventory={'commit':head,'files':files,'bytes_excluding_inventory':sum(f['bytes'] for f in files)}
inventory_bytes=(json.dumps(inventory,indent=2)+'\n').encode('utf-8')
if inventory['bytes_excluding_inventory']+len(inventory_bytes)>5_000_000: raise SystemExit('Complete assembly site including inventory exceeds 5 MB budget')
(SITE/'inventory.json').write_bytes(inventory_bytes)
print(json.dumps({'commit':head,'files':len(files)+1,'bytes_excluding_inventory':sum(f['bytes'] for f in files),'packages':len(packages),'rust_notices':len(rust_notices)}))
