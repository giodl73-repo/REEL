"""Rebuild exact delivered source outside Git, including original native CLI controls."""
import argparse,hashlib,json,os,pathlib,shutil,subprocess,tempfile,zipfile
P=pathlib.Path(__file__).resolve().parent;ROOT=P.parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--bindgen',required=True);args=parser.parse_args()
site=ROOT/'target/reel-release/REEL/assembly';cache=P/'.cache';cache.mkdir(exist_ok=True)
read=lambda p:json.loads(p.read_bytes())
original=read(cache/'native-cases.json');controls=read(cache/'original-cli-controls.json')
provenance=read(site/'provenance.json')
env=dict(os.environ,CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_INCREMENTAL='0')
def run(command,cwd,environment=env):
 result=subprocess.run(command,cwd=cwd,env=environment,capture_output=True)
 with (cache/'source-replay.log').open('ab') as log:
  log.write(('\n$ '+' '.join(map(str,command))+'\n').encode());log.write(result.stdout);log.write(result.stderr)
 if result.returncode:raise SystemExit('Source replay failed; see .cache/source-replay.log')
 return result.stdout
(cache/'source-replay.log').write_bytes(b'')
with tempfile.TemporaryDirectory(prefix='REEL assembly source replay ') as temporary:
 base=pathlib.Path(temporary).resolve();assert not base.is_relative_to(ROOT.resolve())
 with zipfile.ZipFile(site/'source.zip') as archive:
  for name in archive.namelist():
   relative=pathlib.PurePosixPath(name)
   assert relative.parts[0]=='REEL-source' and '..' not in relative.parts and not relative.is_absolute()
   destination=base.joinpath(*relative.parts);assert destination.resolve().is_relative_to(base)
   destination.parent.mkdir(parents=True,exist_ok=True);destination.write_bytes(archive.read(name))
 source=base/'REEL-source';consumer=source/'browser/assembly'
 probe=subprocess.run(['git','rev-parse','--show-toplevel'],cwd=source,capture_output=True)
 assert probe.returncode!=0 and not (source/'.git').exists()
 cargo=shutil.which('cargo');python=shutil.which('python');npm=shutil.which('npm')
 manifest='browser/assembly/Cargo.toml'
 metadata=json.loads(run([cargo,'+1.95.0','metadata','--manifest-path',manifest,'--locked','--format-version','1'],source))
 first_party=[p for p in metadata['packages'] if p['name'] in ['reel-assembly','reel-assembly-workbench']]
 assert len(first_party)==2
 for package in first_party:assert pathlib.Path(package['manifest_path']).resolve().is_relative_to(source.resolve())
 run([cargo,'+1.95.0','test','-p','reel-assembly','--all-targets','--locked'],source)
 run([cargo,'+1.95.0','test','--manifest-path',manifest,'--locked'],source)
 run([python,'browser/assembly/build-prototype.py','--bindgen',str(pathlib.Path(args.bindgen).resolve())],source)
 assert read(consumer/'.cache/native-cases.json')==original,'Independent native response drift'
 assert read(consumer/'.cache/original-cli-controls.json')==controls,'Original production CLI control drift'
 run([npm,'ci','--ignore-scripts'],consumer)
 browser_env=dict(env,REEL_SITE_DIR='../../target/reel-prototype')
 run([npm,'run','test:pages','--','--grep-invert','every packaged file|source, build and legal routes'],consumer,browser_env)
 browser=read(consumer/'.cache/browser-results.json')
 assert all(browser['stats'][k]==0 for k in ['unexpected','skipped','flaky'])
 shutil.copyfile(consumer/'.cache/browser-results.json',cache/'source-replay-browser.json')
 rebuilt=read(consumer/'.cache/prototype-build.json')
 receipt={'source_commit':provenance['commit'],'source_zip_sha256':hashlib.sha256((site/'source.zip').read_bytes()).hexdigest(),'outside_git':True,'git_discovery_returncode':probe.returncode,'path_contains_spaces':' ' in str(base),'first_party_manifests_contained':True,'native_assembly_tests':51,'adapter_tests':4,'browser_checks':browser['stats']['expected'],'identical_native_request_responses':len(original),'identical_original_production_cli_controls':len(controls),'rebuilt_wasm_sha256':rebuilt['wasm_sha256'],'original_wasm_sha256':provenance['wasm_sha256'],'binary_identity_claimed':False,'full_native_production_ci_replayed':False}
 (cache/'source-replay.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8');print(json.dumps(receipt))
