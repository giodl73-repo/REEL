'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const {catalogueFixture,deferred}=require('./ux/catalogue-fixture.cjs');
const calls=f=>f.seen.filter(r=>r.path==='/api/lessons');
const row=(f,name)=>f.nodes(f.get('assets')).find(e=>e.tag==='tr'&&e.children[0]?.children[0]?.textContent===name);
async function pick(f,index){const select=f.nodes().find(e=>e.tag==='select'&&e.getAttribute('aria-label')==='Recorded lesson');select.value=String(index);await select.onchange();await f.settle();}

test('lessons stay hidden and do no evidence IO without explicit enrollment',async()=>{
 const f=catalogueFixture();await f.settle();assert.equal(f.get('lessons').hidden,true);assert.equal(calls(f).length,0);
});
test('real shipped lesson controls show old-job hold, demotion and revert with stable identities',async()=>{
 const f=catalogueFixture({teaching:true});await f.settle();await f.click(f.get('lessons'));
 assert.match(f.text('assets'),/Recorded rehearsal/);assert.match(row(f,'original').children[1].textContent,/Not recorded/);
 await pick(f,3);const hash=row(f,'original').children[0].title;
 assert.match(row(f,'original').children[3].textContent,/Working \/ F/);assert.doesNotMatch(row(f,'original').children[3].textContent,/History \/ D/);
 await f.click('Next lesson');assert.equal(row(f,'original').children[0].title,hash);
 assert.match(row(f,'original').children[1].textContent,/Working \/ F/);assert.match(row(f,'original').children[3].textContent,/History \/ D/);
 assert.doesNotMatch(row(f,'original').children[3].textContent,/Working \/ F/);
 await f.click('Next lesson');assert.equal(row(f,'original').children[0].title,hash);assert.match(row(f,'original').children[3].textContent,/Working \/ F/);
 assert.equal(calls(f).length,1);assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));
 assert.equal(f.seen.filter(r=>r.path.startsWith('/api/storage')).length,0);
});
test('visiting the sample catalogue and returning retains the chosen lesson',async()=>{
 const f=catalogueFixture({teaching:true});await f.settle();await f.click(f.get('lessons'));await pick(f,4);
 await f.click('Inspect sample catalogue');assert.equal(f.state().mode,'works');assert.equal(f.state().project,'atlas');
 await f.click(f.get('lessons'));assert.equal(f.get('category-heading').textContent,'job ended');
});
test('missing observations stay unknown rather than becoming zero copies',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.lessons[4].objects=b.lessons[4].objects.filter(o=>o.name!=='original');return b;});
 await f.settle();await f.click(f.get('lessons'));await pick(f,4);
 assert.match(row(f,'original').children[3].textContent,/Not recorded/);assert.doesNotMatch(row(f,'original').children[3].textContent,/No managed copies/);
});
test('late lesson successes and errors cannot replace a different project view',async()=>{
 for(const fail of [false,true]){
  const f=catalogueFixture({teaching:true}),pending=deferred();f.override('/api/lessons',()=>pending.promise);await f.settle();await f.click(f.get('lessons'));
  await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));
  if(fail)pending.reject(Error('Old lesson failure'));else pending.resolve(f.reply('/api/lessons',{}));await f.settle();
  assert.equal(f.state().project,'boreal');assert.equal(f.state().mode,'works');assert.equal(f.get('notice').hidden,true);assert.doesNotMatch(f.text('assets'),/Recorded rehearsal|Old lesson/);
 }
});
test('unavailable or unrecognized evidence clears old lesson controls and supports explicit retry',async()=>{
 const f=catalogueFixture({teaching:true});await f.settle();await f.click(f.get('lessons'));await pick(f,3);
 f.override('/api/lessons',()=>({fixtureError:'Teaching metadata unavailable'}));await f.context.loadLessons();
 assert.match(f.text('assets'),/unavailable/);assert.equal(f.nodes().some(e=>e.textContent==='Next lesson'),false);
 f.override('/api/lessons',()=>({...f.reply('/api/lessons',{}),authorizes_storage:true}));await f.click('Retry lessons');assert.match(f.get('notice').textContent,/Unrecognized teaching/);
 f.override('/api/lessons',()=>f.reply('/api/lessons',{}));await f.click('Retry lessons');assert.equal(f.get('category-heading').textContent,'merge v2 job active');
});
test('enrolled lesson titles and events stay text, never executable markup',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.lessons[0].event='<img src=x onerror=alert(1)>';return b;});
 await f.settle();await f.click(f.get('lessons'));assert.match(f.text('assets'),/<img src=x/);assert.equal(f.nodes().some(e=>e.tag==='img'),false);
});

test('partial provider coverage shows known copies plus unknown, including the next before column',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-2';
  for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
  const l=b.lessons[3];l.converged=false;l.storage_converged=false;l.holds=['unavailable'];
  for(const o of l.objects){o.observed=o.observed.filter(r=>r!=='backup');o.unknown=['backup'];}
  return b;});
 await f.settle();await f.click(f.get('lessons'));await pick(f,3);
 assert.match(row(f,'original').children[3].textContent,/Working \/ F/);assert.match(row(f,'original').children[3].textContent,/Unknown · Backup \/ M/);
 assert.match(f.get('result-count').textContent,/partial coverage/);await pick(f,4);assert.match(row(f,'original').children[1].textContent,/Unknown · Backup \/ M/);
});

test('unknown-only provider coverage cannot become zero recorded copies',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-2';
  for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
  const l=b.lessons[0];l.converged=false;l.storage_converged=false;l.holds=['unavailable'];
  for(const o of l.objects){o.observed=[];o.unknown=['backup'];}return b;});
 await f.settle();await f.click(f.get('lessons'));assert.match(row(f,'original').children[3].textContent,/Unknown · Backup \/ M/);
 assert.doesNotMatch(row(f,'original').children[3].textContent,/No managed copies/);
});

test('conflicting, unversioned or falsely converged unknown coverage refuses',async()=>{
 for(const bad of ['overlap','foreign','version','converged','missing']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-2';
   for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
   const l=b.lessons[1],o=l.objects[0];l.converged=false;l.storage_converged=false;o.unknown=['backup'];o.observed=o.observed.filter(r=>r!=='backup');
   if(bad==='overlap')o.observed.push('backup');if(bad==='foreign')o.unknown=['elsewhere'];if(bad==='version')b.schema='tig.catalogue-teaching.design-1';if(bad==='converged')l.converged=true;if(bad==='missing')delete o.unknown;
   return b;});await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized teaching coverage/);
 }
});

test('rejected stale attempts distinguish historical desire from unchanged protected copies',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-3';
  for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
  const l=b.lessons[3];l.converged=false;l.basis_current=false;l.storage_converged=false;l.remaining_page=false;l.stream_bytes=0;l.effects=[];
  l.rejection={error:'stale',planning_basis:'a'.repeat(64),observed_basis:'b'.repeat(64)};l.objects.find(o=>o.name==='original').desired=['history','backup'];return b;});
 await f.settle();await f.click(f.get('lessons'));await pick(f,3);
 assert.match(f.text('assets'),/Rejected before storage effects/);assert.match(f.text('assets'),/Current observed basis/);
 assert.match(row(f,'original').children[2].textContent,/History \/ D/);assert.match(row(f,'original').children[3].textContent,/Working \/ F/);
 assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));
});

test('rejection evidence cannot hide effects, convergence, unchanged basis or older protocol versions',async()=>{
 for(const bad of ['version','effects','stream','converged','current','same','page','null']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-3';
   for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
   const l=b.lessons[3];l.converged=false;l.basis_current=false;l.storage_converged=false;l.remaining_page=false;l.stream_bytes=0;l.effects=[];
   l.rejection={error:'stale',planning_basis:'a'.repeat(64),observed_basis:'b'.repeat(64)};
   if(bad==='version')b.schema='tig.catalogue-teaching.design-2';if(bad==='effects')l.effects=[{}];if(bad==='stream')l.stream_bytes=1;if(bad==='converged')l.converged=true;
   if(bad==='current')l.basis_current=true;if(bad==='same')l.rejection.observed_basis=l.rejection.planning_basis;if(bad==='page')l.remaining_page=true;if(bad==='null')l.rejection=null;
   return b;});await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized rejected attempt/);
 }
});

test('final allocation evidence refuses negative unsafe and nonnumeric values',async()=>{
 for(const allocation of [-1,Number.MAX_SAFE_INTEGER+1,'4096']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-4';
   for(const lesson of b.lessons)for(const o of lesson.objects)o.unknown=[];
   const l=b.lessons[3];l.final_loss={phase:'completed',new_removed_bytes:0,historical_removed_bytes:0,synthetic_owner_consent:true,simulated_clock:true,file_allocation_before_new_removals:allocation};
   for(const o of l.objects)o.before=[...o.observed];return b;});
  await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized allocation evidence/);
 }
});

// DTO guard/render tests; the native integration case supplies actual SDK data.
function acquisitionBook(f){
 const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-5';
 for(const l of b.lessons)for(const o of l.objects)o.unknown=[];
 const l=structuredClone(b.lessons.at(-1));l.id='same-hash-reacquired';l.title='Same bytes, new retained birth';
 const original=l.objects.find(o=>o.name==='original');
 for(const o of l.objects){o.before=[...o.observed].sort();o.desired=null;o.acquisition_selection='unchanged';}
 original.before=[];original.observed=['backup'];original.acquisition_selection='new_birth';
 l.stream_bytes=original.object.bytes;l.converged=true;l.storage_converged=false;l.remaining_page=false;l.holds=[];
 l.effects=[{kind:'reacquired',role:'backup',object:original.object,logical_bytes:original.object.bytes}];
 l.reacquisition={phase:'retained',object:original.object,role:'backup',parent_birth:0,new_birth:1,
  added_logical_copy_bytes:original.object.bytes,ack:'process_restart',charged_footprint:4096+original.object.bytes,
  old_plan:'a'.repeat(64),old_retry_new_removed_bytes:0,old_retry_historical_removed_bytes:original.object.bytes*2,
  synthetic_owner_consent:true,simulated_clock:true,retention:'verified_at_recording'};
 b.lessons.push(l);return b;
}

test('reacquisition shows new retained birth and historical retry without storage actions',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>acquisitionBook(f));
 await f.settle();await f.click(f.get('lessons'));await pick(f,7);
 assert.match(f.text('assets'),/Explicit reacquisition/);assert.match(f.text('assets'),/acknowledged across process restart/);
 assert.match(f.text('assets'),/Old deletion retry: 0 B new removal/);assert.match(f.text('assets'),/Conservative accounted footprint/);
 assert.match(row(f,'original').children[1].textContent,/No managed copies recorded/);
 assert.match(row(f,'original').children[2].textContent,/New retained storage birth/);
 assert.match(row(f,'original').children[3].textContent,/Backup \/ M/);
 assert.match(row(f,'fixed').children[2].textContent,/Unchanged/);
 assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));
});

test('reacquisition refuses wrong version consent birth ack charge or new deletion',async()=>{
 for(const bad of ['version','consent','clock','birth','ack','retention','bytes','charge','new_loss','mixed']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=acquisitionBook(f),l=b.lessons.at(-1),r=l.reacquisition;
   if(bad==='version')b.schema='tig.catalogue-teaching.design-4';if(bad==='consent')r.synthetic_owner_consent=false;
   if(bad==='clock')r.simulated_clock=false;if(bad==='birth')r.new_birth=r.parent_birth;if(bad==='ack')r.ack='provider_durable';
   if(bad==='retention')r.retention='unknown';if(bad==='bytes')r.added_logical_copy_bytes++;if(bad==='charge')r.charged_footprint=-1;
   if(bad==='new_loss')r.old_retry_new_removed_bytes=1;
   if(bad==='mixed')l.final_loss={phase:'completed',new_removed_bytes:0,historical_removed_bytes:0,synthetic_owner_consent:true,simulated_clock:true};
   return b;});await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized reacquisition/);
 }
});

// Guard/render DTOs only; native tests separately supply real11-step SDK evidence.
function restoredBook(f){
 const b=acquisitionBook(f);b.schema='tig.catalogue-teaching.design-6';const l=b.lessons.at(-1),r=l.reacquisition;delete l.reacquisition;
 l.id='returned-backup-acquired';l.converged=false;l.storage_converged=false;
 l.restoration={phase:'backup_acquired',object:r.object,parent_birth:r.parent_birth,new_birth:r.new_birth,ack:r.ack,acquired_input_bytes:r.object.bytes,working_stream_bytes:0,charged_footprint:r.charged_footprint,retention:r.retention,old_retry:null,synthetic_owner_consent:true,simulated_clock:true};
 for(const o of l.objects)o.desired=[...o.observed];l.objects.find(o=>o.name==='original').desired=['backup','working'];
 const done=structuredClone(l);done.id='returned-working-restored';done.converged=true;done.storage_converged=true;done.restoration.phase='working_restored';done.restoration.working_stream_bytes=r.object.bytes;done.restoration.old_retry={new_removed_bytes:0,historical_removed_bytes:r.object.bytes*2};
 const o=done.objects.find(o=>o.name==='original');o.before=['backup'];o.observed=['backup','working'];done.effects=[{kind:'reacquired',role:'working',object:r.object,logical_bytes:r.object.bytes}];b.lessons.push(done);return b;
}
test('joined acquisition and restoration show separate streams and historical retry with GET only',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>restoredBook(f));await f.settle();await f.click(f.get('lessons'));await pick(f,7);
 assert.match(f.text('assets'),/Working restoration is still pending/);assert.doesNotMatch(row(f,'original').children[3].textContent,/Working \/ F/);
 await pick(f,8);assert.match(f.text('assets'),/Working restored from retained backup/);assert.match(f.text('assets'),/Completed old deletion retry: 0 B new removal/);assert.match(row(f,'original').children[3].textContent,/Working \/ F/);assert.doesNotMatch(row(f,'original').children[3].textContent,/History \/ D/);assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));
});
test('joined restoration refuses incorrect phase source counters acknowledgement consent or mixed records',async()=>{
 for(const bad of ['version','consent','clock','birth','ack','retention','stream','charge','early_retry','new_loss','converged','mixed']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=restoredBook(f),l=b.lessons[bad==='new_loss'?8:7],r=l.restoration;
   if(bad==='version')b.schema='tig.catalogue-teaching.design-5';if(bad==='consent')r.synthetic_owner_consent=false;if(bad==='clock')r.simulated_clock=false;if(bad==='birth')r.new_birth=r.parent_birth;
   if(bad==='ack')r.ack='provider_durable';if(bad==='retention')r.retention='unknown';if(bad==='stream')r.working_stream_bytes++;if(bad==='charge')r.charged_footprint=-1;
   if(bad==='early_retry')r.old_retry={new_removed_bytes:0,historical_removed_bytes:0};if(bad==='new_loss')r.old_retry.new_removed_bytes=1;if(bad==='converged')l.converged=true;
   if(bad==='mixed')l.final_loss={phase:'completed',new_removed_bytes:0,historical_removed_bytes:0,synthetic_owner_consent:true,simulated_clock:true};return b;
  });await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized joined restoration/);
 }
});


// DTO guard/render cases; remote_teaching.rs supplies actual CLI/provider proof.
function remoteBook(f){
 const b=f.reply('/api/lessons',{});b.schema='tig.catalogue-teaching.design-7';
 const seeds=require('../../../examples/catalogue/lifecycle.design-1.json').objects;
 for(const l of b.lessons)for(const o of l.objects){if(seeds[o.name])o.object={sha256:seeds[o.name].sha256,bytes:seeds[o.name].bytes};o.unknown||=[];}
 const objects=['shared','fixed'].map(n=>({sha256:seeds[n].sha256,bytes:seeds[n].bytes}));
 for(const [i,phase] of ['upload_disabled','payload_published','readback_held','publication_rechecked'].entries()){
  const l=structuredClone(b.lessons[6]);l.id='remote-'+i;l.title='remote '+phase;l.stream_bytes=0;l.effects=[];l.holds=[];l.basis_current=true;l.converged=true;l.storage_converged=true;l.remaining_page=false;
  l.remote_publication={phase,receiver:'e'.repeat(64),objects:i?objects:[],readback_state:i===0?'not_started':i===2?'held':'verified',held_reason:i===2?'integrity':null,fixture_input_staging_bytes:54,put_payload_bytes_this_step:[0,54,0,0][i],put_payload_bytes_total:i?54:0,put_count_total:i?2:0,readback_payload_bytes_this_step:[0,108,54,108][i],http_requests_this_step:[0,16,4,8][i],client_body_io_bytes:[null,3484,null,1100][i],retained_custody:false,removal_authority:false,synthetic_endpoint:true};b.lessons.push(l);
 }
 return b;
}
test('remote publication shows disabled verified held and rechecked facts with GET only',async()=>{
 const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>remoteBook(f));await f.settle();await f.click(f.get('lessons'));
 for(const [i,text] of [[7,'payload upload disabled'],[8,'publication verified'],[9,'current remote availability is unknown'],[10,'no additional PUTs']]){await pick(f,i);assert.match(f.text('assets'),/Recorded fixture receiver R/);assert.ok(f.text('assets').includes(text));assert.match(f.text('assets'),/Fixture input staging: 54 B/);assert.match(f.text('assets'),/grants no removal authority/);}
 assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));assert.equal(calls(f).length,1);
});
test('remote publication refuses invented custody IO state objects and local effects',async()=>{
 for(const bad of ['version','custody','authority','synthetic','receiver','put_step','put_total','body_io','readback','held_reason','unknown_object','local_stream','local_effect','mixed']){
  const f=catalogueFixture({teaching:true});f.override('/api/lessons',()=>{const b=remoteBook(f),l=b.lessons[8],r=l.remote_publication;
   if(bad==='version')b.schema='tig.catalogue-teaching.design-6';if(bad==='custody')r.retained_custody=true;if(bad==='authority')r.removal_authority=true;if(bad==='synthetic')r.synthetic_endpoint=false;if(bad==='receiver')r.receiver='bad';
   if(bad==='put_step')r.put_payload_bytes_this_step++;if(bad==='put_total')r.put_payload_bytes_total++;if(bad==='body_io')r.client_body_io_bytes=-1;if(bad==='readback')r.readback_state='held';if(bad==='held_reason')r.held_reason='integrity';
   if(bad==='unknown_object')r.objects[0].sha256='f'.repeat(64);if(bad==='local_stream')l.stream_bytes=54;if(bad==='local_effect')l.effects=[{}];if(bad==='mixed')l.restoration={phase:'backup_acquired'};return b;
  });await f.settle();await f.click(f.get('lessons'));assert.match(f.get('notice').textContent,/Unrecognized (remote publication|joined restoration)/);
 }
});
