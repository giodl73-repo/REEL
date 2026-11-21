'use strict';
const vm=require('node:vm'),{readFileSync}=require('node:fs'),{join}=require('node:path'),{createHash}=require('node:crypto');
const {documentFixture}=require('./dom.cjs');
const web=join(__dirname,'../../src/catalogue/web'),html=readFileSync(join(web,'index.html'),'utf8');
const clone=value=>JSON.parse(JSON.stringify(value));
function reference(id,project='atlas'){const raw=Buffer.from('fixture/'+project+'/'+id);return {id,object:{sha256:createHash('sha256').update(raw).digest('hex'),bytes:raw.length}};}
function catalogueFixture({count=2,partial=false,ruleCount=1,historical=false,selectedBytes=false,objectMode=false,teaching=false}={}){
 const dom=documentFixture(html),seen=[],overrides=new Map();
 const registered=partial?Math.max(count+2,ruleCount):count+2;
 const commit=project=>(project==='atlas'?'a':'b').repeat(40),basis=project=>(project==='atlas'?'c':'d').repeat(64);
 const payload=reference('payload','shared').object;
 const retained=()=>({slot:'history/v1',role:'result',group:'history',object:payload});
 const card=(id,project='atlas')=>({project,id,title:id==='source'?'Source board':id==='board'?'Planning board':'Shared output title',definition:'type/item',category:'items',icon:'document',status:{lifecycle:'draft'},selected:selectedBytes||(objectMode&&id==='source')?payload:null,streams:objectMode&&id==='source'&&project==='boreal'?1:0});
 const work=(collection='sequences',project='atlas')=>({project,collection,id:collection==='sequences'?'sequence-01':'board-01',title:collection==='sequences'?'Fixture sequence':'Fixture board',category:collection,state:collection==='sequences'?'Accepted':'Ready',version:'v2',facets:[],icon:'folder',member_count:3});
 const collections=[['sequences','Sequences','Accepted'],['boards','Boards','Ready']].map(([id,label,status])=>({id,label,count_label:status+' '+label.toLowerCase(),count_state:status,count_value:1,count:1,states:{[status]:1},facets:{},state_facets:{},top_level:true}));
 const projects=['atlas','boreal'].map(id=>({id,label:id==='atlas'?'Atlas':'Boreal',commit:commit(id),collections:clone(collections),work_count:2,records:registered,selected_records:selectedBytes?registered:objectMode?1:0,unique_selected_bytes:selectedBytes||objectMode?payload.bytes:0,categories:{items:registered},refs:[['refs/heads/main',commit(id)]],storage_locations:objectMode?2:0,storage_available:objectMode}));
 const rows=project=>Array.from({length:count},(_,n)=>{const id='render-'+String(n+1).padStart(2,'0'),asset=reference(id,project),target=reference('source',project);if(historical&&n===0)asset.object=reference(id+'/previous',project).object;return {asset,class:'output',distance:1,path:[{from:asset,target,kind:'composition',pointer:'/inputs/0'}],target,navigation:historical&&n===0?null:card(id,project)};});
 function reply(path,q){
  const project=q.project||'atlas';
  if(path==='/api/projects')return {projects:clone(projects),default_project:'atlas',teaching_available:teaching};
  if(path==='/api/lessons'&&teaching){
   const corpus=JSON.parse(readFileSync(join(__dirname,'../../../../examples/catalogue/lifecycle.design-1.json'),'utf8'));
   return {schema:'tig.catalogue-teaching.design-1',basis:reference('book').object,project:'atlas',roles:[{id:'working',label:'Working / F'},{id:'history',label:'History / D'},{id:'backup',label:'Backup / M'}],mode:'recorded-rehearsal',synthetic_domains:true,independent_hardware_backup_proven:false,authorizes_storage:false,pending:['final retirement','metadata lifetime'],
    lessons:corpus.stages.slice(0,7).map((stage,index)=>({id:stage.id,title:stage.id.replaceAll('-',' '),event:stage.event,report:reference(stage.id).object,plan:'e'.repeat(64),source_commit:commit('atlas'),source_available:true,source_sequence:index,converged:true,basis_current:true,stream_bytes:0,holds:[],effects:[],objects:Object.entries(corpus.objects).map(([name,object])=>({name,object:{sha256:object.sha256,bytes:object.bytes},desired:stage.expected_copies[name].length?stage.expected_copies[name]:null,observed:stage.expected_copies[name]}))}))};
  }
  if(path==='/api/works'){const selected=q.collections?q.collections.split(','):q.collection?[q.collection]:collections.map(c=>c.id),items=selected.map(c=>work(c,project));return {snapshots:{[project]:commit(project)},items,total:items.length};}
  if(path==='/api/work'){const w=work(q.collection,project);return {commit:commit(project),work:w,root:card(q.collection==='sequences'?'source':'board',project),parents:[],children_total:0,versions:[{label:'v2',current:true}],deliveries:[],members:[card('source',project)],member_total:1,member_categories:{items:1}};}
  if(path==='/api/asset')return {commit:commit(project),record:reference(q.id,project),definition:reference('type/item','shared'),card:card(q.id,project),fields:[],streams:objectMode&&q.id==='source'&&project==='boreal'?[retained()]:[],storage_locations:objectMode?2:0};
  if(path==='/api/storage/objects'){
   const binding=(project,slot='selected')=>({project,id:'source',title:'Source board',category:'items',slot,role:slot==='selected'?'selected':'result',usage:slot==='selected'?'selected-subject':'previous-revision',version:slot==='selected'?null:'v1',selected:payload,status:{lifecycle:'draft'},nested:null});
   const row={sha256:payload.sha256,title:'Shared fixture bytes',bytes:payload.bytes,basis:'e'.repeat(64),binding_count:3,bindings:[binding('atlas'),binding('boreal'),binding('boreal','history/v1')],copies:[{cache_id:'working',bytes:payload.bytes,allocated_bytes:null,observed_at:null},{cache_id:'archive',bytes:payload.bytes,allocated_bytes:null,observed_at:null}],primary:[],anomalies:[],stored_file_bytes:payload.bytes*2,matching_copy_bytes:payload.bytes*2};
   return {snapshots:{atlas:commit('atlas'),boreal:commit('boreal')},scope:q.work?{title:'Fixture sequence',registered_records:1,nested:null,nested_available:false,nested_roots:null}:null,copy_summary:[{cache_id:'working',file_bytes:payload.bytes,objects:1},{cache_id:'archive',file_bytes:payload.bytes,objects:1}],caches:[{id:'working',label:'Working copy',root:'/fixture/working'},{id:'archive',label:'Archive copy',root:'/fixture/archive'}],items:[row],total:1,unique_content_bytes:payload.bytes,stored_file_bytes:payload.bytes*2,kinds:['items'],anomaly_counts:{},limitations:'Fixture observations only; custody and complete consumers unknown.'};
  }
  if(path==='/api/compare'){
   const side=prefix=>({selector:{project:q[prefix+'_project'],id:q[prefix+'_id']},record:reference(q[prefix+'_id'],q[prefix+'_project']),definition:reference('type/item','shared'),selected:card(q[prefix+'_id'],q[prefix+'_project']).selected});
   const same=q.left_project===q.right_project&&q.left_id===q.right_id,leftHistory=objectMode&&q.left_project==='boreal'&&q.left_id==='source',rightHistory=objectMode&&q.right_project==='boreal'&&q.right_id==='source';
   return {schema:'tig.catalogue-compare.experimental-1',status:'complete',snapshots:{[q.left_project]:commit(q.left_project),[q.right_project]:commit(q.right_project)},basis_sha256:basis(q.left_project),coverage:{complete_declared_scope:true,complete_project_scope:false,payloads_opened:0},data:{left:side('left'),right:side('right'),record_equal:same,definition_equal:true,selected_equal:selectedBytes||(objectMode&&q.left_id==='source'&&q.right_id==='source')?true:null,authored_changed_fields:same?[]:['language'],streams:{added:rightHistory&&!leftHistory?[retained()]:[],removed:leftHistory&&!rightHistory?[retained()]:[],changed:[],unchanged:leftHistory&&rightHistory?1:0}},authorizes_eviction:false};
  }
  if(path==='/api/impact'){
   const known=!partial&&q.id==='source'&&q.kinds.split(',').includes('composition')?rows(project):[],offset=Number(q.offset||0),items=known.slice(offset,offset+25),next=offset+items.length;
   return {schema:'tig.catalogue-impact.experimental-1',project,commit:commit(project),basis_sha256:basis(project),status:partial?'partial':'complete',coverage:{complete_declared_scope:!partial,complete_project_scope:false,payloads_opened:0},paging:{offset,returned:items.length,total_known:known.length,next_offset:next<known.length?next:null},data:{target:reference(q.id,project),items,total_known:known.length,gap_count:partial?registered:0,gaps:partial?Array.from({length:Math.min(registered,100)},(_,i)=>({at:reference(i===0?'source':'unresolved-'+i,project),reason:'Unadmitted'})):[],gaps_truncated:partial&&registered>100,unadmitted_definition_count:partial?ruleCount:0,unadmitted_definitions:partial?Array.from({length:Math.min(ruleCount,100)},(_,i)=>({definition:reference('type/missing-'+i,project),registered_records:i===0?registered-ruleCount+1:1})):[]},authorizes_eviction:false};
  }
  if(path==='/api/assets')return {snapshots:{[project]:commit(project)},items:[card('source',project)],total:1};
  throw Error('Unexpected fixture request: '+path);
 }
 const context=vm.createContext({document:dom.document,window:{scrollTo(){}},inspector:dom.get('inspector'),Option:dom.Option,URLSearchParams,Intl,setTimeout,clearTimeout,
  fetch:async(url,init)=>{const [path,query]=url.split('?'),q=Object.fromEntries(new URLSearchParams(query));seen.push({path,q,init});const response=await (overrides.has(path)?overrides.get(path)(q):reply(path,q));return {ok:response?.fixtureError==null,json:async()=>clone(response?.fixtureError?{error:response.fixtureError}:response)};}});
 const scripts=[...html.matchAll(/<script\b[^>]*><\/script>/g)],files=[...html.matchAll(/<script src="\/([a-z-]+\.js)" defer><\/script>/g)];
 if(!files.length||files.length!==scripts.length)throw Error('Fixture needs an explicit loader for every shipped script declaration');
 for(const [,file] of files)vm.runInContext(readFileSync(join(web,file),'utf8'),context,{filename:file});
 const state=()=>vm.runInContext('state',context),settle=async()=>{for(let n=0;n<8;n++)await new Promise(resolve=>setImmediate(resolve));};
 const nodes=(root=dom.body)=>dom.walk(root).filter(dom.visible),button=label=>{const matches=nodes().filter(e=>e.tag==='button'&&(e.textContent===label||e.getAttribute('aria-label')===label));if(matches.length!==1)throw Error('Expected one button '+label+'; found '+matches.length);return matches[0];};
 const click=async(target,event)=>{await dom.click(typeof target==='string'?button(target):target,event);await settle();};
 return {...dom,context,seen,projects,state,settle,button,nodes,click,reply,override:(path,fn)=>overrides.set(path,fn),text:(id='detail')=>dom.get(id).textContent,work,reference,card};
}
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
module.exports={catalogueFixture,deferred};
