'use strict';
const globalDisk={domain:'',project:'',data:null,offset:0,rows:[]};
async function changeStorageFamily(family,d){
 const p=currentProject(),gen=++state.generation;const previous={family:disk.family,owner:disk.owner,version:disk.version};
 disk.owner='';disk.version='';disk.family=family;disk.offset=0;
 if((d.source.kind==='registered-object-metadata'&&d.source.family!==family)||(family&&!d.families.includes(family))){
  notice('Loading '+(family||'all families')+' from the registered project objects…');
  try{await api('/api/storage/scope',{project:p.id,snapshot:p.commit,family});if(gen!==state.generation)return;}
  catch(e){if(gen===state.generation){Object.assign(disk,previous);drawDisk(d);notice('The broader observation was not loaded: '+e.message);}return;}
 }
 await loadStorage();
}
async function openProjectStorage(id){
 window.scrollTo(0,0);close();state.project=id;state.mode='storage';Object.assign(disk,{project:id,cache:'',family:'',owner:'',version:'',status:'',q:'',offset:0,live:null});
 drawNavigation();const p=currentProject();
 {
  const gen=++state.generation;notice('Checking named registered files for '+p.label+'…');
  try{await api('/api/storage/scope',{project:id,snapshot:p.commit,family:''});if(gen!==state.generation)return;}
  catch(e){notice(e.message);return;}
 }
 await loadStorage();
}
async function loadGlobalStorage(more=false){
 const gen=++state.generation;notice('');
 if(!more){globalDisk.offset=0;globalDisk.rows=[];$('assets').replaceChildren(node('p','empty','Reading storage locations across enrolled projects…'));}
 try{
  const d=await api('/api/storage/global',{project:globalDisk.project,domain:globalDisk.domain,measure:'true',offset:globalDisk.offset});
  if(gen!==state.generation||state.mode!=='storage'||disk.project)return;
  if(Object.entries(d.snapshots).some(([id,commit])=>state.projects.find(p=>p.id===id)?.commit!==commit))throw new Error('A project snapshot changed. Refresh the catalog first.');
  globalDisk.rows.push(...d.objects);globalDisk.data=d;drawGlobalStorage(d);
 }catch(e){if(gen===state.generation)notice(e.message);}
}
function drawGlobalStorage(d){
 const area=$('assets');area.replaceChildren();const stats=document.querySelector('.stats');stats.replaceChildren();
 [[d.domains.length,'enrolled drives / capacity domains'],[d.caches.length,'configured cache locations'],[fmt.format(d.summary.file_copies),'observed file copies · partial'],[bytes(d.summary.recorded_file_bytes),'observed file bytes · partial']].forEach(([v,label])=>{const e=node('div');e.append(node('span','',v),node('small','',label));stats.append(e);});
 const controls=node('div','disk-filters');const back=node('button','button secondary','← Back to objects');back.onclick=()=>openObjectManager(globalDisk.project);controls.append(back);const change=(key,value)=>{globalDisk[key]=value;loadGlobalStorage();};
 controls.append(diskSelect('Drive',globalDisk.domain,[['','All drives'],...d.domains.map(v=>[v.id,v.label])],v=>change('domain',v)),diskSelect('Referenced by project',globalDisk.project,[['','All projects'],...d.projects.map(p=>[p.id,p.label])],v=>change('project',v)));
 const refresh=node('button','button secondary','Refresh drive capacities');refresh.onclick=()=>loadGlobalStorage();controls.append(refresh);area.append(controls);
 const inventory=node('button','button','Refresh registered assets across projects');inventory.onclick=async()=>{
  const gen=state.generation,failures=[];inventory.disabled=true;
  for(const p of d.projects.filter(p=>p.storage_enabled)){
   if(gen!==state.generation)return;notice('Checking named registered files for '+p.label+'; no media reads…');
   try{await api('/api/storage/observe',{project:p.id,snapshot:d.snapshots[p.id],family:''});}catch(e){failures.push(p.label+': '+e.message);}
  }
  if(gen!==state.generation)return;await loadGlobalStorage();if(failures.length)notice('Some observations were not refreshed: '+failures.join('; '));
 };controls.append(inventory);
 const drives=node('section','global-drives');
 d.domains.filter(v=>!globalDisk.domain||v.id===globalDisk.domain).forEach(v=>{
  const card=node('button','global-drive');card.setAttribute('aria-label','Filter drive '+v.label);card.onclick=()=>change('domain',globalDisk.domain===v.id?'':v.id);
  const title=node('div','global-drive-title');title.append(node('strong','',v.label),node('span','muted',v.cache_count+' cache locations'));card.append(title);
  if(v.capacity_measured){
   card.append(node('p','',bytes(v.free)+' free of '+bytes(v.total)));
   const bar=node('div','disk-volume'),used=node('span');used.style.width=(100*(1-v.free/v.total))+'%';used.style.background=v.free/v.total<.05?'#b45a4e':v.free/v.total<.1?'#cf9456':'#61826e';bar.append(used);card.append(bar);
  }else card.append(node('p','muted',v.live_probe_allowed?'Live capacity unavailable':'Live capacity not probed'));
  card.append(node('p','muted',v.totals?bytes(v.totals.recorded_file_bytes)+' observed file bytes · '+fmt.format(v.totals.file_copies)+' copies':'Cache content not observed in the loaded sources'));
  drives.append(card);
 });area.append(drives);
 area.append(node('p','callout','Drive bars show live volume usage. Cache counts below cover loaded observations only; they do not account for the entire drive. Shared cache paths count once across projects. Project reference totals overlap, and different paths can still be aliases.'));
 const projects=node('section','global-projects');projects.append(node('h2','','Projects referencing storage'));
 d.projects.forEach(p=>{
  const row=node('div','global-project');const info=node('div');info.append(node('strong','',p.label),node('p','muted',fmt.format(p.declared_objects)+' unique registered subjects / streams · '+(p.has_observation?'observation loaded':'no cache observation loaded')));
  if(p.totals)info.append(node('span','',bytes(p.totals.recorded_file_bytes)+' observed copies referenced'));
  const inspect=node('button','button secondary','Manage objects');inspect.disabled=!p.storage_enabled;inspect.onclick=()=>openObjectManager(p.id);const compare=node('button','button secondary','Compare works');compare.disabled=!p.storage_enabled;compare.onclick=()=>openProjectStorage(p.id);row.append(info,inspect,compare);projects.append(row);
 });area.append(projects);
 const locations=node('section','global-locations');locations.append(node('h2','','Cache locations'));
 d.caches.forEach(c=>{
  const row=node('div','global-cache');row.append(node('strong','mono',c.root),node('p','',c.enrolled_projects.map(projectLabel).join(' · ')),node('p','muted',c.totals?bytes(c.totals.recorded_file_bytes)+' observed file bytes · '+fmt.format(c.totals.file_copies)+' copies':'No matching observed copies; coverage is unknown'));
  if(c.resolved_root)row.append(node('small','muted','Resolved location: '+c.resolved_root));c.observations.forEach(o=>row.append(node('small','muted',projectLabel(o.project)+' · '+(o.observed_at?new Date(o.observed_at*1000).toLocaleString():'time unknown')+' · '+(o.source.family||o.source.kind))));locations.append(row);
 });area.append(locations);
 const table=node('table','global-object-table');const head=node('tr');['Object','Drive','File bytes','Referencing projects'].forEach(t=>head.append(node('th','',t)));table.append(head);
 globalDisk.rows.forEach(o=>{const row=node('tr');row.append(node('td','mono',o.sha256.slice(0,16)+'…'),node('td','',d.domains.find(v=>v.id===o.domain)?.label||'Unmapped'),node('td','',bytes(o.bytes)),node('td','',o.referencing_projects.map(projectLabel).join(', ')||'Unattributed'));table.append(row);});area.append(table);
 if(globalDisk.rows.length<d.objects_total){const more=node('button','button secondary','More observed file copies');more.onclick=()=>{globalDisk.offset=globalDisk.rows.length;loadGlobalStorage(true);};area.append(more);}
 $('result-count').textContent=fmt.format(d.objects_total)+' observed file copies';document.querySelector('main footer').textContent=d.limitations;
}
