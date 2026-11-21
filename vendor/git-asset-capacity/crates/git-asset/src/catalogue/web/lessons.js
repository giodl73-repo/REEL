'use strict';
const lessonView={book:null,selection:'',active:false};
function resetLessonChrome(){if(lessonView.active){lessonView.active=false;document.querySelector('main footer').textContent='Counts describe declared records and selected objects, not videos produced or physical disk usage. No automatic eviction.';}}
function openLessons(){
 if(!state.teachingAvailable)return;
 state.mode='lessons';state.generation++;close();drawNavigation();load();
}
function drawLessonsNavigation(){
 lessonView.active=true;
 $('heading').textContent='Learn git-asset';$('subheading').textContent='Follow a recorded fixture from Git changes to verified cache copies.';
 $('projects').replaceChildren(navButton('All projects',state.projects.length,false,()=>chooseProject('all'),'◫'));
 state.projects.forEach(p=>$('projects').append(navButton(p.label,p.work_count,false,()=>chooseProject(p.id))));
 $('categories').replaceChildren();$('technical').hidden=true;document.querySelector('.toolbar').hidden=true;
 $('work-controls').hidden=false;$('assets').className='lesson-view';$('more').hidden=true;
 document.querySelector('main footer').textContent='Recorded synthetic rehearsal. Current custody, hardware independence and storage permission are separate.';
}
function lessonRoles(book,values,empty,unknown=[]){
 if(values==null)return node('span','muted','Not recorded');
 const area=node('div','lesson-role-set');
 for(const role of book.roles){if(values.includes(role.id))area.append(node('span','copy-chip',role.label));else if(unknown.includes(role.id))area.append(node('span','copy-chip','Unknown · '+role.label));}
 if(!values.length&&!unknown.length)area.append(node('span','muted',empty));return area;
}
function selectLesson(book,index){
 if(state.mode!=='lessons'||lessonView.book!==book)return;
 const lesson=book.lessons[index];if(!lesson)return;lessonView.selection=lesson.id;
 const controls=$('work-controls');controls.replaceChildren();
 const select=node('select');select.setAttribute('aria-label','Recorded lesson');
 book.lessons.forEach((l,i)=>select.append(new Option((i+1)+'. '+l.title,String(i))));select.value=String(index);
 select.onchange=()=>selectLesson(book,Number(select.value));
 const previous=node('button','button secondary','Previous lesson'),next=node('button','button','Next lesson');
 previous.disabled=index===0;next.disabled=index===book.lessons.length-1;
 previous.onclick=()=>selectLesson(book,index-1);next.onclick=()=>selectLesson(book,index+1);
 const browse=node('button','button secondary','Inspect sample catalogue');browse.onclick=()=>chooseProject(book.project);
 controls.append(previous,select,next,browse);
 const before=book.lessons[index-1],all=new Map();
 for(const l of book.lessons)for(const o of l.objects)all.set(o.name,o.object);
 let knownCopies=0,copyBytes=0;for(const o of lesson.objects)if(Array.isArray(o.observed)){knownCopies+=o.observed.length;copyBytes+=o.object.bytes*o.observed.length;}
 const partial=lesson.objects.some(o=>o.unknown?.length),stats=document.querySelector('.stats');stats.replaceChildren();
 for(const [value,label] of [[(index+1)+' / '+book.lessons.length,'recorded checkpoint'],[all.size,'stable byte identities'],[knownCopies,lesson.remote_publication?'recorded local copies':partial?'known recorded copies':'recorded copies'],[bytes(lesson.stream_bytes),lesson.remote_publication?'local placement streamed':'payload streamed this step']]){
  const s=node('div');s.append(node('span','',value),node('small','',label));stats.append(s);
 }
 $('category-heading').textContent=lesson.title;$('result-count').textContent=bytes(copyBytes)+(partial?' known recorded copy bytes · partial coverage':' recorded copy bytes');
 const area=$('assets');area.replaceChildren();
 area.append(node('p','callout','Recorded rehearsal · synthetic admitted domains. These are saved verified observations, not a check of current hardware custody.'));
 area.append(node('p','lesson-event',lesson.event));
 const source=node('p','muted','Associated Git commit '+lesson.source_commit.slice(0,12)+(lesson.source_available?' · available in this catalogue':' · source metadata unavailable here')+'. Drafts and jobs may contribute separate admitted snapshots.');
 source.title=lesson.source_commit;area.append(source);
 if(lesson.final_loss)area.append(node('p','callout','Explicit final loss · '+lesson.final_loss.phase+'. New removal: '+bytes(lesson.final_loss.new_removed_bytes)+'. Historical deletion: '+bytes(lesson.final_loss.historical_removed_bytes)+'. Separate synthetic owner consent and simulated clock.'+(lesson.final_loss.required_age_millis===undefined?'':' Required continuous inactive age: '+lesson.final_loss.required_age_millis+' ms (simulated).')+' This recorded lesson grants no storage authority.'));
 if(lesson.final_loss?.file_allocation_before_new_removals!==undefined)area.append(node('p','muted','Allocated file data before new removals: '+bytes(lesson.final_loss.file_allocation_before_new_removals)+'. Free-space change: unmeasured.'));
 if(lesson.reacquisition){const r=lesson.reacquisition;
  area.append(node('p','callout','Explicit reacquisition · same bytes, new storage birth '+r.new_birth+' after retired birth '+r.parent_birth+'. Added copy: '+bytes(r.added_logical_copy_bytes)+'. Retention verified at recording; acknowledged across process restart. Separate synthetic owner consent and simulated clock.'));
  area.append(node('p','muted','Old deletion retry: '+bytes(r.old_retry_new_removed_bytes)+' new removal; '+bytes(r.old_retry_historical_removed_bytes)+' historical deletion. Conservative accounted footprint: '+bytes(r.charged_footprint)+'. Net growth and free-space change: unmeasured.'));
 }
 if(lesson.restoration){const r=lesson.restoration;
  area.append(node('p','callout',r.phase==='backup_acquired'?'Explicit backup acquisition · same bytes, new retained backup birth '+r.new_birth+'. Working restoration is still pending.':'Working restored from retained backup · history remains deliberately absent.'));
  area.append(node('p','muted','Explicit input stream: '+bytes(r.acquired_input_bytes)+'. Ordinary working stream: '+bytes(r.working_stream_bytes)+'. Retention verified at recording; process-restart acknowledgement. Synthetic owner consent and clock.'));
  area.append(node('p','muted','Conservative acquisition footprint: '+bytes(r.charged_footprint)+'. Net growth and free-space change: unmeasured.'));
  if(r.old_retry)area.append(node('p','callout','Completed old deletion retry: '+bytes(r.old_retry.new_removed_bytes)+' new removal; '+bytes(r.old_retry.historical_removed_bytes)+' historical deletion. Returned source and current copies preserved.'));
 }
 if(lesson.remote_publication){const r=lesson.remote_publication;
  const labels={upload_disabled:'Git metadata exchanged · payload upload disabled. Receiver contains no payloads.',payload_published:'Explicit remote payload publication verified. Retained custody remains unqualified.',readback_held:'Remote readback held · integrity. Recorded receiver bytes remain; current remote availability is unknown.',publication_rechecked:'Same remote publication rechecked · no additional PUTs. Retained custody remains unqualified.'};
  area.append(node('p','callout',labels[r.phase]));
  area.append(node('p','muted','Fixture input staging: '+bytes(r.fixture_input_staging_bytes)+'. Upload PUT payload this step: '+bytes(r.put_payload_bytes_this_step)+'. Total PUT payload: '+bytes(r.put_payload_bytes_total)+'.'));
  area.append(node('p','muted','Client body IO: '+(r.client_body_io_bytes===null?'not reported for refused operations':bytes(r.client_body_io_bytes)+' including protocol JSON and readbacks')+'. Receiver GET response payload this step: '+bytes(r.readback_payload_bytes_this_step)+'. Free-space change: unmeasured.'));
  const receiver=node('div','lesson-evidence');receiver.append(node('h3','','Recorded fixture receiver R'));
  if(!r.objects.length)receiver.append(node('p','muted','No receiver payloads recorded.'));
  for(const object of r.objects){const local=lesson.objects.find(o=>o.object.sha256===object.sha256);receiver.append(node('p','',local.name+' · '+object.sha256.slice(0,12)+' · '+bytes(object.bytes)));}
  receiver.append(node('p','muted','Remote interface: '+r.readback_state+'. This receiver is separate from the F/D/M placement roles and grants no removal authority.'));area.append(receiver);
 }
 if(!lesson.final_loss&&lesson.rejection)area.append(node('p','callout','Rejected before storage effects: stale plan. The desired column belongs to the rejected plan. A fresh plan is required for current protection.'));
 else if(!lesson.final_loss&&!lesson.restoration&&(!lesson.converged||!lesson.basis_current||lesson.holds.length))area.append(node('p','callout','This recorded step remained pending or held: '+(lesson.holds.join(', ')||'the admitted basis changed')));
 const wrap=node('div','object-table-wrap'),table=node('table','object-manager-table lesson-object-table'),head=node('thead'),header=node('tr');
 for(const text of ['Same bytes','Before this step',lesson.final_loss?'Final selection':lesson.reacquisition?'Recorded acquisition':'Desired in this plan','Observed after'])header.append(node('th','',text));head.append(header);table.append(head);
 const body=node('tbody');
 for(const [name,object] of all){
  const current=lesson.objects.find(o=>o.name===name),old=before?.objects.find(o=>o.name===name),row=node('tr');
  const identity=node('td');identity.append(node('strong','',name),node('small','',bytes(object.bytes)),node('div','mono',object.sha256.slice(0,12)));identity.title=object.sha256;
  const left=node('td'),desired=node('td'),after=node('td');
  left.append(lessonRoles(book,current?.before??old?.observed,'No managed copies recorded',current?.before?[]:old?.unknown||[]));
  if(current?.desired==null)desired.append(node('span','muted',lesson.final_loss?(current.final_selection==='eligible'?'Eligible for separately authorized final removal':current.final_selection==='unselected'?'Not selected in this block':'Protected · excluded from final loss'):lesson.reacquisition?(current.acquisition_selection==='new_birth'?'New retained storage birth':'Unchanged'):'Not included in this recorded plan'));
  else desired.append(lessonRoles(book,current.desired,'No placements required in this plan'));
  after.append(lessonRoles(book,current?.observed,'No managed copies recorded',current?.unknown||[]));
  row.append(identity,left,desired,after);body.append(row);
 }
 table.append(body);wrap.append(table);area.append(wrap);
 const evidence=node('details','lesson-evidence');evidence.append(node('summary','','Recorded evidence'));
 evidence.append(node('p','muted','Report '+lesson.report.sha256),node('p','muted','Plan '+lesson.plan));
 if(lesson.rejection)evidence.append(node('p','muted','Planned protection basis '+lesson.rejection.planning_basis),node('p','muted','Current observed basis '+lesson.rejection.observed_basis));
 const effects=node('ul');for(const effect of lesson.effects)effects.append(node('li','',effect.kind.replaceAll('_',' ')+' · '+(book.roles.find(r=>r.id===effect.role)?.label||effect.role)+' · '+effect.object.sha256.slice(0,12)+' · '+bytes(effect.logical_bytes)));
 if(!lesson.effects.length)evidence.append(node('p','muted','No new provider effects recorded.'));else evidence.append(effects);area.append(evidence);
 if(book.pending.length)area.append(node('p','muted','Remaining course work: '+book.pending.join(' · ')));
}
async function loadLessons(){
 const gen=++state.generation;notice('');lessonView.book=null;$('more').hidden=true;
 $('work-controls').replaceChildren();document.querySelector('.stats').replaceChildren();$('result-count').textContent='';
 $('assets').replaceChildren(node('div','empty','Reading hash-pinned lesson reports…'));
 try{
  const book=await api('/api/lessons');if(gen!==state.generation||state.mode!=='lessons')return;
  if(!['tig.catalogue-teaching.design-1','tig.catalogue-teaching.design-2','tig.catalogue-teaching.design-3','tig.catalogue-teaching.design-4','tig.catalogue-teaching.design-5','tig.catalogue-teaching.design-6','tig.catalogue-teaching.design-7'].includes(book.schema)||book.mode!=='recorded-rehearsal'||book.authorizes_storage!==false||book.synthetic_domains!==true||book.independent_hardware_backup_proven!==false
   ||!state.projects.some(p=>p.id===book.project)||!Array.isArray(book.lessons)||!book.lessons.length||book.lessons.length>16||!Array.isArray(book.roles)||book.roles.length<2||book.roles.length>4
   ||book.lessons.some(l=>!Array.isArray(l.objects)||l.objects.length>32||!Array.isArray(l.holds)||!Array.isArray(l.effects)))throw Error('Unrecognized teaching response');
  const roles=new Set(book.roles.map(r=>r.id));
  for(const l of book.lessons)for(const o of l.objects){
   if(book.schema==='tig.catalogue-teaching.design-1'){if(o.unknown!==undefined)throw Error('Unrecognized teaching coverage');}
   else if(!Array.isArray(o.unknown)||o.unknown.some((r,i)=>!roles.has(r)||(i&&o.unknown[i-1]>=r)||o.observed?.includes(r))||(o.unknown.length&&(l.converged||l.storage_converged===true)))throw Error('Unrecognized teaching coverage');
  }
  for(const l of book.lessons)if(l.rejection!==undefined){const r=l.rejection;
   if(!['tig.catalogue-teaching.design-3','tig.catalogue-teaching.design-4','tig.catalogue-teaching.design-5','tig.catalogue-teaching.design-6'].includes(book.schema)||!r||r.error!=='stale'||typeof r.planning_basis!=='string'||typeof r.observed_basis!=='string'||!r.planning_basis.match(/^[0-9a-f]{64}$/)||!r.observed_basis.match(/^[0-9a-f]{64}$/)||r.planning_basis===r.observed_basis||l.basis_current!==false||l.converged!==false||l.storage_converged!==false||l.stream_bytes!==0||l.effects.length||l.remaining_page)throw Error('Unrecognized rejected attempt');
  }
  for(const l of book.lessons)if(l.final_loss!==undefined){const r=l.final_loss;
   if(r?.file_allocation_before_new_removals!==undefined&&(!Number.isSafeInteger(r.file_allocation_before_new_removals)||r.file_allocation_before_new_removals<0))throw Error('Unrecognized allocation evidence');
   if(!['tig.catalogue-teaching.design-4','tig.catalogue-teaching.design-5','tig.catalogue-teaching.design-6'].includes(book.schema)||!r||!['completed','cancelled'].includes(r.phase)||r.synthetic_owner_consent!==true||r.simulated_clock!==true||!Number.isSafeInteger(r.new_removed_bytes)||r.new_removed_bytes<0||!Number.isSafeInteger(r.historical_removed_bytes)||r.historical_removed_bytes<r.new_removed_bytes||l.objects.some(o=>!Array.isArray(o.before)||o.before.some((id,i)=>!roles.has(id)||(i&&o.before[i-1]>=id))))throw Error('Unrecognized final loss');
  }
  for(const l of book.lessons)if(l.reacquisition!==undefined){const r=l.reacquisition;
   if(!['tig.catalogue-teaching.design-5','tig.catalogue-teaching.design-6'].includes(book.schema)||!r||l.final_loss||l.rejection||r.phase!=='retained'||r.retention!=='verified_at_recording'||r.ack!=='process_restart'||!roles.has(r.role)||r.synthetic_owner_consent!==true||r.simulated_clock!==true
    ||!Number.isSafeInteger(r.parent_birth)||r.parent_birth<0||!Number.isSafeInteger(r.new_birth)||r.new_birth!==r.parent_birth+1
    ||!Number.isSafeInteger(r.added_logical_copy_bytes)||r.added_logical_copy_bytes<1||r.object?.bytes!==r.added_logical_copy_bytes||!r.object.sha256?.match(/^[0-9a-f]{64}$/)
    ||!Number.isSafeInteger(r.charged_footprint)||r.charged_footprint<r.added_logical_copy_bytes||r.old_retry_new_removed_bytes!==0||!Number.isSafeInteger(r.old_retry_historical_removed_bytes)||r.old_retry_historical_removed_bytes<0
    ||l.stream_bytes!==r.added_logical_copy_bytes||l.converged!==true||l.storage_converged!==false||l.remaining_page||l.objects.some(o=>!Array.isArray(o.before)||o.before.some((id,i)=>!roles.has(id)||(i&&o.before[i-1]>=id))))throw Error('Unrecognized reacquisition');
  }
  for(const l of book.lessons)if(l.restoration!==undefined){const r=l.restoration,done=r?.phase==='working_restored';
   if(book.schema!=='tig.catalogue-teaching.design-6'||!r||!['backup_acquired','working_restored'].includes(r.phase)||l.final_loss||l.reacquisition||l.rejection
    ||r.synthetic_owner_consent!==true||r.simulated_clock!==true||r.retention!=='verified_at_recording'||r.ack!=='process_restart'
    ||!Number.isSafeInteger(r.parent_birth)||r.parent_birth<0||!Number.isSafeInteger(r.new_birth)||r.new_birth!==r.parent_birth+1
    ||!Number.isSafeInteger(r.acquired_input_bytes)||r.acquired_input_bytes<1||r.object?.bytes!==r.acquired_input_bytes||!r.object.sha256?.match(/^[0-9a-f]{64}$/)
    ||!Number.isSafeInteger(r.charged_footprint)||r.charged_footprint<r.acquired_input_bytes||r.working_stream_bytes!==(done?r.acquired_input_bytes:0)
    ||l.converged!==done||l.storage_converged!==done||l.basis_current!==true||l.remaining_page||l.stream_bytes!==r.acquired_input_bytes
    ||(done&&(!r.old_retry||r.old_retry.new_removed_bytes!==0||!Number.isSafeInteger(r.old_retry.historical_removed_bytes)||r.old_retry.historical_removed_bytes<0||l.holds.length))
    ||(!done&&r.old_retry!==null)||l.objects.some(o=>!Array.isArray(o.before)||o.before.some((id,i)=>!roles.has(id)||(i&&o.before[i-1]>=id))))throw Error('Unrecognized joined restoration');
  }
  for(const l of book.lessons)if(l.remote_publication!==undefined){const r=l.remote_publication,phases=['upload_disabled','payload_published','readback_held','publication_rechecked'],i=phases.indexOf(r?.phase),put=[0,54,0,0],get=[0,108,54,108],requests=[0,16,4,8];
   if(book.schema!=='tig.catalogue-teaching.design-7'||i<0||l.final_loss||l.reacquisition||l.restoration||l.rejection||r.retained_custody!==false||r.removal_authority!==false||r.synthetic_endpoint!==true||!r.receiver?.match(/^[0-9a-f]{64}$/)
    ||r.fixture_input_staging_bytes!==54||r.put_payload_bytes_this_step!==put[i]||r.put_payload_bytes_total!==(i?54:0)||r.put_count_total!==(i?2:0)||r.readback_payload_bytes_this_step!==get[i]||r.http_requests_this_step!==requests[i]
    ||r.readback_state!==(i===0?'not_started':i===2?'held':'verified')||r.held_reason!==(i===2?'integrity':null)
    ||([0,2].includes(i)?r.client_body_io_bytes!==null:!Number.isSafeInteger(r.client_body_io_bytes)||r.client_body_io_bytes<54||r.client_body_io_bytes>131072)
    ||!Array.isArray(r.objects)||r.objects.length!==(i?2:0)||r.objects.some((o,n)=>!o.sha256?.match(/^[0-9a-f]{64}$/)||!Number.isSafeInteger(o.bytes)||o.bytes<1||!l.objects.some(v=>v.object.sha256===o.sha256&&v.object.bytes===o.bytes)||(n&&r.objects[n-1].sha256>=o.sha256))||(i&&r.objects.reduce((n,o)=>n+o.bytes,0)!==54)
    ||!l.basis_current||l.converged!==true||l.storage_converged!==true||l.stream_bytes!==0||l.effects.length||l.holds.length||l.remaining_page)throw Error('Unrecognized remote publication');
  }
  lessonView.book=book;const selected=book.lessons.findIndex(l=>l.id===lessonView.selection);selectLesson(book,Math.max(0,selected));
 }catch(e){if(gen===state.generation&&state.mode==='lessons'){
  lessonView.book=null;notice(e.message);$('assets').replaceChildren(node('div','empty','Recorded lessons are unavailable. No storage changes were requested.'));
  const retry=node('button','button secondary','Retry lessons');retry.onclick=()=>{if(state.mode==='lessons')loadLessons();};$('work-controls').replaceChildren(retry);
 }}
}
