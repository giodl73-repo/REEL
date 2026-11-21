'use strict';
// Consumes actual native producer/API output, never an invented placement DTO.
const {readFileSync}=require('node:fs'),assert=require('node:assert/strict');
const {catalogueFixture}=require('./catalogue-fixture.cjs');
(async()=>{
 const book=JSON.parse(readFileSync(0,'utf8')),f=catalogueFixture({teaching:true});
 await f.settle();
 f.state().projects.push({...f.state().projects[0],id:book.project,label:'Native sample'});
 f.override('/api/lessons',()=>book);await f.click(f.get('lessons'));
 assert.equal(f.state().mode,'lessons');assert.equal(f.get('notice').hidden,true);
 const row=name=>f.nodes(f.get('assets')).find(e=>e.tag==='tr'&&e.children[0]?.children[0]?.textContent===name);
 const selector=()=>f.nodes().find(e=>e.tag==='select'&&e.getAttribute('aria-label')==='Recorded lesson');
 assert.equal(selector().children.length,book.lessons.length);
 let identity;
 for(const [id,role] of [[book.lessons.some(l=>l.id==='merge-backup-offline')?'merge-backup-offline':'merge-v2-job-active','working'],['job-ended','history'],['revert-v1','working']]){
  const index=book.lessons.findIndex(l=>l.id===id);assert.ok(index>=0);
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  const original=row('original'),expected=book.roles.find(r=>r.id===role).label;
  assert.ok(original.children[3].textContent.includes(expected));
  if(identity)assert.equal(original.children[0].title,identity);
  identity=original.children[0].title;
 }
 if(book.lessons.some(l=>l.id==='publish-v2')){
  for(const [index,role] of [[7,'history'],[8,'working'],[9,'history']]){
   const select=selector();select.value=String(index);await select.onchange();await f.settle();
   assert.ok(row('original').children[3].textContent.includes(book.roles.find(r=>r.id===role).label));
   assert.equal(row('original').children[0].title,identity);
  }
 }
 if(book.lessons.some(l=>l.id==='worktree-open-v1')){
  for(const [index,role] of [[7,'working'],[8,'working'],[9,'working'],[10,'working'],[11,'history']]){
   const select=selector();select.value=String(index);await select.onchange();await f.settle();
   assert.ok(row('original').children[3].textContent.includes(book.roles.find(r=>r.id===role).label));
   assert.equal(row('original').children[0].title,identity);
  }
  assert.ok(f.text('assets').includes('Associated Git commit'));
  assert.ok(f.text('assets').includes('separate admitted snapshots'));
 }
 if(book.lessons.some(l=>l.id==='merge-backup-offline')){
  for(const id of ['merge-backup-offline','job-ended-backup-offline']){
   const select=selector();select.value=String(book.lessons.findIndex(l=>l.id===id));await select.onchange();await f.settle();
   assert.ok(row('original').children[3].textContent.includes('Unknown · Backup / M'));
   assert.ok(row('original').children[3].textContent.includes('Working / F'));
   assert.ok(f.text('assets').includes('pending or held'));
   assert.ok(f.get('result-count').textContent.includes('partial coverage'));
  }
  const select=selector();select.value=String(book.lessons.findIndex(l=>l.id==='backup-catchup'));await select.onchange();await f.settle();
  assert.ok(row('original').children[1].textContent.includes('Unknown · Backup / M'));
  assert.ok(row('original').children[3].textContent.includes('History / D'));
  assert.ok(row('original').children[3].textContent.includes('Backup / M'));
  assert.ok(!row('original').children[3].textContent.includes('Working / F'));
 }
 if(book.lessons.some(l=>l.id==='stale-plan-rejected')){
  for(const [id,role] of [['stale-plan-rejected','working'],['fresh-plan-job-active','working'],['fresh-plan-job-ended','history']]){
   const select=selector();select.value=String(book.lessons.findIndex(l=>l.id===id));await select.onchange();await f.settle();
   assert.ok(row('original').children[3].textContent.includes(book.roles.find(r=>r.id===role).label));
   assert.equal(row('original').children[0].title,identity);
   if(id==='stale-plan-rejected'){
    assert.ok(f.text('assets').includes('Rejected before storage effects'));
    assert.ok(row('original').children[2].textContent.includes('History / D'));
    assert.ok(!row('original').children[2].textContent.includes('Working / F'));
    assert.ok(f.text('assets').includes('Current observed basis'));
   }
  }
 }
 if(book.lessons.some(l=>l.id==='same-byte-rename')){
  const index=book.lessons.findIndex(l=>l.id==='same-byte-rename');
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.equal(row('original').children[0].title,identity);
  for(const name of ['original','fixed','shared']){
   const object=book.lessons[index].objects.find(o=>o.name===name);
   for(const role of object.observed)assert.ok(row(name).children[3].textContent.includes(book.roles.find(r=>r.id===role).label));
  }
  assert.ok(f.text('assets').includes('No new provider effects recorded.'));
  assert.deepEqual(book.lessons[index].effects,[]);
 }
 for(const [index,lesson] of book.lessons.entries())if(lesson.final_loss){
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.ok(f.text('assets').includes('Explicit final loss · '+lesson.final_loss.phase));
  if(lesson.final_loss.file_allocation_before_new_removals!==undefined){assert.ok(f.text('assets').includes('Allocated file data before new removals:'));assert.ok(f.text('assets').includes('Free-space change: unmeasured.'));}
  if(lesson.final_loss.required_age_millis!==undefined)assert.ok(f.text('assets').includes('Required continuous inactive age: '+lesson.final_loss.required_age_millis+' ms (simulated).'));
  assert.ok(row('original').children[1].textContent.includes('Backup / M'));
  assert.ok(row('original').children[2].textContent.includes('Eligible for separately authorized final removal'));
  const fixed=lesson.objects.find(o=>o.name==='fixed');
  assert.ok(row('fixed').children[2].textContent.includes(fixed.final_selection==='unselected'?'Not selected in this block':'Protected'));
  if(lesson.final_loss.phase==='completed')assert.ok(row('original').children[3].textContent.includes('No managed copies recorded'));
 }
 for(const [index,lesson] of book.lessons.entries())if(lesson.reacquisition){
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.ok(f.text('assets').includes('Explicit reacquisition'));
  assert.ok(f.text('assets').includes('Retention verified at recording'));
  assert.ok(f.text('assets').includes('Old deletion retry: 0 B new removal'));
  assert.ok(row('original').children[1].textContent.includes('No managed copies recorded'));
  assert.ok(row('original').children[2].textContent.includes('New retained storage birth'));
  assert.ok(row('original').children[3].textContent.includes('Backup / M'));
  assert.ok(!row('original').children[3].textContent.includes('Working / F'));
  assert.ok(row('fixed').children[2].textContent.includes('Unchanged'));
 }
 for(const [index,lesson] of book.lessons.entries())if(lesson.id==='git-return-after-final-loss'){
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.ok(f.text('assets').includes('final_retirement'));
  assert.ok(f.text('assets').includes('payload remains absent'));
  assert.ok(row('original').children[1].textContent.includes('No managed copies recorded'));
  assert.ok(row('original').children[2].textContent.includes('Working / F'));
  assert.ok(row('original').children[2].textContent.includes('Backup / M'));
  assert.ok(row('original').children[3].textContent.includes('No managed copies recorded'));
  assert.ok(!row('original').children[3].textContent.includes('Unknown'));
  assert.ok(row('fixed').children[3].textContent.includes('History / D'));
  assert.ok(row('shared').children[3].textContent.includes('Working / F'));
  assert.equal(row('original').children[0].title,identity);
 }
 for(const [index,lesson] of book.lessons.entries())if(lesson.restoration){
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.equal(f.get('notice').hidden,true);assert.ok(f.text('assets').includes('Explicit input stream: 29 B'));
  assert.ok(row('original').children[3].textContent.includes('Backup / M'));
  if(lesson.restoration.phase==='backup_acquired'){
   assert.ok(f.text('assets').includes('Working restoration is still pending'));
   assert.ok(!row('original').children[3].textContent.includes('Working / F'));
  }else{
   assert.ok(f.text('assets').includes('Completed old deletion retry: 0 B new removal'));
   assert.ok(row('original').children[3].textContent.includes('Working / F'));
  }
  assert.ok(!row('original').children[3].textContent.includes('History / D'));
  assert.equal(row('original').children[0].title,identity);
 }
 for(const [index,lesson] of book.lessons.entries())if(lesson.remote_publication){
  const select=selector();select.value=String(index);await select.onchange();await f.settle();
  assert.ok(f.text('assets').includes('Recorded fixture receiver R'));
  assert.ok(f.text('assets').includes('Fixture input staging: 54 B'));
  assert.ok(f.text('assets').includes('grants no removal authority'));
  const r=lesson.remote_publication;
  if(r.phase==='upload_disabled')assert.ok(f.text('assets').includes('payload upload disabled'));
  if(r.phase==='payload_published')assert.ok(f.text('assets').includes('publication verified'));
  if(r.phase==='readback_held')assert.ok(f.text('assets').includes('current remote availability is unknown'));
  if(r.phase==='publication_rechecked')assert.ok(f.text('assets').includes('no additional PUTs'));
  assert.ok(row('original').children[3].textContent.includes('History / D'));
  assert.ok(!row('original').children[3].textContent.includes('Working / F'));
  assert.equal(row('original').children[0].title,identity);
 }
 assert.equal(f.seen.filter(r=>r.path==='/api/lessons').length,1);
 assert.ok(f.seen.every(r=>!r.init?.method||r.init.method==='GET'));
 process.stdout.write('Actual native evidence renders job hold, demotion and revert.\n');
})().catch(error=>{process.stderr.write(String(error)+'\n');process.exitCode=1;});
