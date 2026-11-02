"""Build private qualification output; source/legal packaging is a separate gate."""
import argparse,copy,hashlib,json,os,pathlib,shutil,subprocess
P=pathlib.Path(__file__).resolve().parent;ROOT=P.parents[1]
parser=argparse.ArgumentParser();parser.add_argument('--bindgen',required=True);args=parser.parse_args()
assert subprocess.check_output([args.bindgen,'--version']).decode().strip()=='wasm-bindgen 0.2.127'
target=pathlib.Path(os.environ['CARGO_TARGET_DIR']);assert target.is_absolute()
env=dict(os.environ,CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_INCREMENTAL='0')
home=pathlib.Path(os.environ.get('CARGO_HOME',str(pathlib.Path.home()/'.cargo'))).resolve()
env.pop('RUSTFLAGS',None)
env['CARGO_ENCODED_RUSTFLAGS']='\x1f'.join(['--remap-path-prefix='+str(ROOT)+'=/reel','--remap-path-prefix='+str(home)+'=/cargo'])
subprocess.run([shutil.which('python'),str(P/'verify-dependencies.py')],cwd=ROOT,env=env,check=True)
for command in [
 ['cargo','+1.95.0','build','--manifest-path','browser/assembly/Cargo.toml','--locked','--release','--target','wasm32-unknown-unknown','--lib'],
 ['cargo','+1.95.0','build','--manifest-path','browser/assembly/Cargo.toml','--locked','--bin','reel-assembly-workbench-cli'],
 ['cargo','+1.95.0','build','--locked','--bin','reel'],
]:
 command_env=dict(env)
 if '--target' in command:command_env['CARGO_ENCODED_RUSTFLAGS']+='\x1f-C\x1flink-arg=--max-memory=67108864'
 subprocess.run(command,cwd=ROOT,env=command_env,check=True)
site=ROOT/'target/reel-prototype/REEL/assembly';site.mkdir(parents=True,exist_ok=True)
for name in ['index.html','style.css','app.js','worker.js']:shutil.copyfile(P/'web'/name,site/name)
shutil.copyfile(P/'fixtures/graph.json',site/'graph.json')
subprocess.run([args.bindgen,'--target','web','--remove-name-section','--out-dir',str(site/'pkg'),'--out-name','reel_assembly_workbench',str(target/'wasm32-unknown-unknown/release/reel_assembly_workbench.wasm')],check=True)
cache=P/'.cache';cache.mkdir(exist_ok=True)
suffix='.exe' if os.name=='nt' else ''
# Isolate executable bytes from the shared Cargo target before running controls.
for name in ['reel-assembly-workbench-cli','reel']:
 shutil.copy2(target/'debug'/(name+suffix),cache/(name+suffix))
 if os.name!='nt':assert os.access(cache/(name+suffix),os.X_OK),name
cli=cache/('reel-assembly-workbench-cli'+suffix);original_cli=cache/('reel'+suffix)
fixture=json.loads((P/'fixtures/graph.json').read_text(encoding='utf-8'))
pointer=json.loads((P/'fixtures/pointer.json').read_text(encoding='utf-8'))
requests=[]
def add(graph=fixture,target='episode',kind='node',selected=None):
 requests.append({'graph_text':graph if isinstance(graph,str) else json.dumps(graph),'target':target,'kind':kind,'pointer_text':'' if selected is None else json.dumps(selected)})
add();add(selected=pointer);add(target='review',kind='presentation');add(target='review',kind='presentation',selected=pointer)
for target_id in ['scene','','missing','x'*257]:add(target=target_id)
for text in ['','{}','null','{',' '*131073,'\ufeff'+json.dumps(fixture),json.dumps(fixture,indent=2).replace('\n','\r\n')]:add(text)
for mode in ['','Node','renderer']:add(kind=mode)
for field in ['logical_id','sha256']:
 v=copy.deepcopy(pointer);v['selected_lock'][field]='different' if field=='logical_id' else 'f'*64;add(selected=v)
v=copy.deepcopy(pointer);v['extra']=True;add(selected=v)
for key in ['schema','extra']:
 v=copy.deepcopy(fixture);v[key]='unknown';add(v)
v=copy.deepcopy(fixture);v['nodes'][0]['inputs']=['episode'];add(v)
v=copy.deepcopy(fixture);v['nodes'].append({'id':'unused-cycle','inputs':['unused-cycle'],'slots':[],'events':[]});add(v)
v=copy.deepcopy(fixture);v['nodes'][0]['inputs']=['missing'];add(v)
v=copy.deepcopy(fixture);v['nodes'].append(v['nodes'][0]);add(v)
v=copy.deepcopy(fixture);v['nodes']=[v['nodes'][0]]*129;add(v)
v=copy.deepcopy(fixture);v['nodes'][1]['inputs']=['scene']*1025;add(v)
for value in [-1,2,1e308]:
 v=copy.deepcopy(fixture);v['events'][0]['phrase_start_seconds']=value;add(v)
v=copy.deepcopy(fixture)
for slot in v['slots']:slot['disposition']='unselected';slot.pop('selected_revision_id',None);slot['revisions']=[]
add(v)
v=copy.deepcopy(fixture);v['slots'][0]['revisions'][0]['asset']['cache_uri']='https://example.invalid/private.png';add(v)
v=copy.deepcopy(fixture);v['nodes']=[];v['presentation_targets']=[];add(v)
v=copy.deepcopy(fixture);v['nodes'][0]['id']='<img src=x onerror=alert(1)>';add(v,target='scene')
receipts=[]
for request in requests:
 result=subprocess.run([str(cli)],input=json.dumps(request).encode(),capture_output=True,check=True)
 receipts.append({'request':request,'output':result.stdout.decode().rstrip('\r\n')})
(cache/'native-cases.json').write_text(json.dumps(receipts,indent=2)+'\n',encoding='utf-8')
# The unmodified production CLI independently verifies selected node closures.
controls=[];control_root=cache/'original-controls';control_root.mkdir(exist_ok=True)
for number,request in enumerate([requests[1],requests[3],requests[23],requests[24],requests[25],requests[26],requests[32]]):
 case=control_root/str(number);case.mkdir(exist_ok=True)
 graph=json.loads(request['graph_text']);selected=copy.deepcopy(pointer);selected['selected_lock']=graph['lock']
 (case/'graph.json').write_text(json.dumps(graph),encoding='utf-8');(case/'pointer.json').write_text(json.dumps(selected),encoding='utf-8')
 command=[str(original_cli),'semantic-assembly-check','--graph',str(case/'graph.json'),'--pointer',str(case/'pointer.json'),'--target',request['target'] if request['kind']=='node' else 'episode','--output-path',str(case/'closure.json')]
 result=subprocess.run(command,cwd=ROOT,capture_output=True)
 current=dict(request,kind='node',target=command[command.index('--target')+1],pointer_text=json.dumps(selected))
 adapter=json.loads(subprocess.run([str(cli)],input=json.dumps(current).encode(),capture_output=True,check=True).stdout)
 if result.returncode==0:
  native=json.loads((case/'closure.json').read_bytes());assert adapter['selected']==native
 else:
  assert adapter['ok'] is False
  assert adapter['error'] in result.stderr.decode(),(adapter,result.stderr.decode())
 controls.append({'request':current,'native_exit_code':result.returncode,'native_stdout':result.stdout.decode(),'native_stderr':result.stderr.decode(),'expected_selected':json.loads((case/'closure.json').read_bytes()) if result.returncode==0 else None})
(cache/'original-cli-controls.json').write_text(json.dumps(controls,indent=2)+'\n',encoding='utf-8')
wasm=site/'pkg/reel_assembly_workbench_bg.wasm'
receipt={'prototype_only':True,'native_cases':len(receipts),'original_cli_controls':len(controls),'wasm_bytes':wasm.stat().st_size,'wasm_sha256':hashlib.sha256(wasm.read_bytes()).hexdigest(),'original_cli_sha256':hashlib.sha256(original_cli.read_bytes()).hexdigest()}
(cache/'prototype-build.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
# QA artifacts retain hashes/controls, not extra native binaries with different notice closures.
for executable in [cli,original_cli]:
 assert executable.parent.resolve()==cache.resolve()
 executable.unlink()
print(json.dumps(receipt))
