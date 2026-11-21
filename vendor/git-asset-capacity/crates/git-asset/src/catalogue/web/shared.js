'use strict';
const sharedView={left:'',right:'',category:''};
function openShared(){
 const ids=state.projects.map(p=>p.id);if(!ids.includes(sharedView.left))sharedView.left=state.project!=='all'?state.project:ids[0]||'';
 if(!ids.includes(sharedView.right)||sharedView.right===sharedView.left)sharedView.right=ids.find(id=>id!==sharedView.left)||'';
 state.mode='shared';state.q='';state.generation++;close();drawNavigation();load();
}
function drawSharedNavigation(){
 $('heading').textContent='Shared assets';$('subheading').textContent='Compare recorded selected bytes across two exact project snapshots.';
 $('projects').replaceChildren(navButton('All projects',state.projects.length,false,()=>chooseProject('all'),'◫'));
 state.projects.forEach(p=>$('projects').append(navButton(p.label,p.work_count,false,()=>chooseProject(p.id))));
 $('categories').replaceChildren();$('technical').hidden=true;document.querySelector('.toolbar').hidden=true;
 document.querySelector('.stats').replaceChildren();$('assets').className='asset-grid';
 const controls=$('work-controls');controls.replaceChildren();
 for(const side of ['left','right']){
  const label=node('label','snapshot-label',side==='left'?'First project':'Second project'),select=node('select');
  select.setAttribute('aria-label',side==='left'?'First comparison project':'Second comparison project');
  state.projects.forEach(p=>select.append(new Option(p.label,p.id)));select.value=sharedView[side];
  select.onchange=()=>{sharedView[side]=select.value;sharedView.category='';drawSharedNavigation();load();};label.append(select);controls.append(label);
 }
 const categories=[...new Set(state.projects.filter(p=>[sharedView.left,sharedView.right].includes(p.id)).flatMap(p=>Object.keys(p.categories)))].sort();
 const label=node('label','snapshot-label','Category'),select=node('select');select.setAttribute('aria-label','Shared asset category');
 select.append(new Option('All registered categories',''));categories.forEach(c=>select.append(new Option(c,c)));select.value=sharedView.category;
 select.onchange=()=>{sharedView.category=select.value;load();};label.append(select);controls.append(label);
 const compare=node('button','button secondary','Compare selected bytes');compare.onclick=()=>load();controls.append(compare);
 document.querySelector('main footer').textContent='Recorded byte identity only. Unregistered files, nested inventories and other streams are excluded; no eviction authority.';
}
function sharedReference(project,reference){
 const b=node('button','member-button',projectLabel(project)+' · '+reference.asset);
 b.onclick=()=>inspect({project,id:reference.asset,title:reference.asset,category:reference.category,definition:reference.definition});return b;
}
function sharedObjectCard(item,left,right){
 const card=node('article','asset-card'),body=node('div','card-body');
 body.append(node('h3','',bytes(item.object.bytes)),node('div','mono',item.object.sha256));
 for(const side of ['left','right']){
  const project=side==='left'?left:right;body.append(node('h4','',projectLabel(project)+' · '+fmt.format(item[side+'_total'])+' records'));
  item[side].forEach(ref=>body.append(sharedReference(project,ref)));
  if(item[side].length<item[side+'_total'])body.append(node('p','muted','First 100 references shown; CLI report includes the full selected-object list.'));
 }
 card.append(body);return card;
}
async function loadShared(more=false){
 const gen=++state.generation;notice('');if(!more){state.offset=0;state.rows=[];document.querySelector('.stats').replaceChildren();$('result-count').textContent='';$('assets').replaceChildren(node('div','empty','Comparing recorded identities…'));}
 $('more').hidden=true;$('category-heading').textContent='Shared selected objects';
 try{
  const {left,right,category}=sharedView;
  if(!left||!right||left===right)throw Error('Choose two different projects.');
  const data=await api('/api/shared',{left,right,category,offset:state.offset});if(gen!==state.generation)return;
  if(data.coverage.some(c=>state.projects.find(p=>p.id===c.project)?.commit!==c.commit))throw Error('Project snapshot changed. Refresh the catalogue before comparing.');
  state.rows.push(...data.items);state.offset=state.rows.length;
  $('assets').replaceChildren(...state.rows.map(item=>sharedObjectCard(item,left,right)));
  if(!state.rows.length)$('assets').append(node('div','empty','No shared selected objects in this registered scope. Unindexed files have not been compared.'));
  $('result-count').textContent=fmt.format(data.shared_objects)+' unique shared objects · '+bytes(data.shared_unique_bytes);
  const stats=document.querySelector('.stats');stats.replaceChildren();
  data.coverage.forEach(c=>{const panel=node('div');panel.append(node('span','',fmt.format(c.matched_records)),node('small','',projectLabel(c.project)+' records in scope · '+c.without_selected_object+' without selection'),node('small','mono','Pinned '+c.commit.slice(0,10)));stats.append(panel);});
  $('more').hidden=state.offset>=data.shared_objects;
 }catch(e){if(gen!==state.generation)return;notice(e.message);$('result-count').textContent='Comparison unavailable';$('assets').replaceChildren(node('div','empty','Adjust the selection and compare again.'));}
}
