'use strict';
const impactKinds=['production','composition','publication','evidence','authority'];
function impactCurrent(asset,view,sequence){return state.selected===asset&&state.tab==='Impact'&&asset.impactView===view&&view.sequence===sequence&&asset.details?.commit===view.commit;}
async function traceImpact(asset,view,more=false){
 if(more&&view.next==null)return;
 const sequence=++view.sequence,offset=more?view.next:0,basis=more?view.basis:'';
 view.error='';view.loading=true;
 if(!more){view.rows=[];view.report=null;view.next=null;view.basis='';}
 drawDetail();
 try{
  if(!view.kinds.length)throw Error('Choose at least one relationship kind.');
  const report=await api('/api/impact',{project:asset.project,id:asset.id,snapshot:view.commit,kinds:view.kinds.join(','),offset,basis});
  if(!impactCurrent(asset,view,sequence))return;
  if(report.project!==asset.project||report.commit!==view.commit)throw Error('Snapshot changed. Refresh this project before tracing again.');
  if(more&&report.basis_sha256!==basis)throw Error('Relationship interpretation changed. Trace again from the beginning.');
  const expected=asset.details.record,actual=report.data.target;
  if(actual.id!==expected.id||actual.object.sha256!==expected.object.sha256||actual.object.bytes!==expected.object.bytes)throw Error('Record revision changed. Refresh the asset before tracing again.');
  view.rows.push(...report.data.items);view.report=report;view.basis=report.basis_sha256;view.next=report.paging.next_offset;
 }catch(e){if(impactCurrent(asset,view,sequence)){view.error=e.message==='stale_basis'?'Refresh this project and reopen the asset before tracing again. (stale_basis)':e.message;view.rows=[];view.report=null;view.next=null;view.basis='';}}
 finally{if(impactCurrent(asset,view,sequence)){view.loading=false;drawDetail();document.getElementById('impact-feedback')?.focus();}else if(view.sequence===sequence)view.loading=false;}
}
async function openImpactConsumer(previous,card){
 const back=$('back-work'),origin={hidden:back.hidden,textContent:back.textContent,onclick:back.onclick};
 await inspect(card);
 if(state.selected!==card)return;
 back.hidden=false;back.textContent='← Back to impact';
 back.onclick=async()=>{await inspect(previous,'Impact');if(state.selected===previous)Object.assign(back,origin);};
}
function impactConsumer(asset,row){
 const box=node('article','stream');
 if(row.navigation){const link=node('button','member-button',row.navigation.title||row.asset.id);link.setAttribute('aria-label','Inspect '+(row.navigation.title||row.asset.id)+' ('+row.asset.id+')');link.onclick=()=>openImpactConsumer(asset,row.navigation);box.append(link,node('div','mono',row.asset.id));}
 else box.append(node('strong','',row.asset.id),node('p','muted','Historical revision; opening the current record would show different bytes.'));
 box.append(node('p','',row.class+' · '+row.distance+' relationship '+(row.distance===1?'step':'steps')),node('div','mono',row.asset.object.sha256));
 const details=node('details'),summary=node('summary','','View exact dependency path ('+row.path.length+' '+(row.path.length===1?'step':'steps')+')'),list=node('ol');
 row.path.forEach(edge=>{const item=node('li');item.append(node('strong','',edge.from.id+' → '+edge.target.id),node('p','',edge.kind+' · '+edge.pointer),node('small','mono',edge.from.object.sha256+' → '+edge.target.object.sha256));list.append(item);});
 details.append(summary,list);box.append(details);return box;
}
function drawImpact(asset,area){
 let view=asset.impactView;
 if(!view||view.commit!==asset.details.commit)view=asset.impactView={commit:asset.details.commit,kinds:impactKinds.slice(0,3),sequence:0,rows:[],report:null,next:null,basis:'',loading:false,error:''};
 const choices=node('fieldset');choices.append(node('legend','','Relationship kinds'));
 impactKinds.forEach(kind=>{
  const label=node('label','snapshot-label'),input=node('input');input.type='checkbox';input.id='impact-'+kind;input.checked=view.kinds.includes(kind);
  input.onchange=()=>{view.sequence++;view.loading=false;view.kinds=impactKinds.filter(k=>k===kind?input.checked:view.kinds.includes(k));view.rows=[];view.report=null;view.next=null;view.basis='';view.error='';drawDetail();document.getElementById(input.id)?.focus();};
  label.append(input,node('span','',kind));choices.append(label);
 });
 const run=node('button','button',view.loading?'Tracing…':'Trace consumers');run.disabled=view.loading;run.onclick=()=>traceImpact(asset,view);
 area.append(node('p','muted','Find records that could be affected if this exact asset revision changes. Choose which declared relationships to follow.'),choices,run);
 const results=node('div');results.id='impact-feedback';results.tabIndex=-1;results.setAttribute('aria-live','polite');area.append(results);
 if(view.error){results.append(node('p','callout',view.error));return;}
 if(view.loading){results.append(node('p','muted','Resolving admitted relationships in the pinned snapshot…'));return;}
 if(!view.report){results.append(node('p','muted','Choose relationships and trace consumers to see paths and coverage.'));return;}
 const report=view.report,data=report.data,complete=report.status==='complete';
 results.append(node('h3','',fmt.format(data.total_known)+' known '+(data.total_known===1?'consumer':'consumers')),
  node('p','muted','Showing '+fmt.format(view.rows.length)+' of '+fmt.format(data.total_known)+' known consumers.'),node('small','mono','Snapshot '+report.commit),
  node('p',complete?'muted':'callout',complete?'Declared relationship coverage resolved within the chosen kinds.':'Coverage incomplete. Known paths are useful; a zero count does not establish that this asset is unused.'));
 view.rows.forEach(row=>results.append(impactConsumer(asset,row)));
 if(!view.rows.length)results.append(node('p','muted',complete?'No declared consumers in this snapshot for the chosen relationship kinds.':'No known consumers; unresolved relationships remain.'));
 if(data.gap_count){const gaps=node('details'),list=node('ul');gaps.append(node('summary','','Unresolved relationships ('+fmt.format(data.gap_count)+')'));(data.gaps||[]).forEach(g=>{const item=node('li');item.append(node('span','',g.reason+' · '+g.at.id),node('small','mono',g.at.object.sha256));list.append(item);});gaps.append(list);if(data.gaps_truncated)gaps.append(node('p','muted','First 100 gaps shown. The total includes unresolved entries beyond this sample.'));results.append(gaps);}
 if(data.unadmitted_definition_count){const missing=node('details'),list=node('ul');missing.append(node('summary','','Relationship rules needed ('+fmt.format(data.unadmitted_definition_count)+' '+(data.unadmitted_definition_count===1?'definition':'definitions')+')'));(data.unadmitted_definitions||[]).forEach(d=>{const item=node('li');item.append(node('strong','',d.definition.id),node('p','',d.registered_records+' registered records'),node('small','mono',d.definition.object.sha256+' · '+d.definition.object.bytes+' B'));list.append(item);});if((data.unadmitted_definitions||[]).length<data.unadmitted_definition_count)missing.append(node('p','muted','Showing '+(data.unadmitted_definitions||[]).length+' of '+data.unadmitted_definition_count+' missing definition pins.'));missing.append(list,node('p','muted','Admit relationship rules from the authoritative schema. Missing rules cannot establish absence.'));results.append(missing);}
 if(view.next!=null){const more=node('button','button secondary','Show more consumers');more.onclick=()=>traceImpact(asset,view,true);results.append(more);}
 results.append(node('p','muted','Potential impact is a triage list. Producer and delivery checks determine whether regeneration is needed. These results do not authorize cache eviction.'));
}
