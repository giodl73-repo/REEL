'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const {catalogueFixture,deferred}=require('./ux/catalogue-fixture.cjs');
async function openImpact(f){await f.settle();await f.click('Open Fixture sequence');await f.click('Trace impact of source record');}
const calls=(f,path)=>f.seen.filter(r=>r.path===path);

test('reopening the same record rejects an older successful inspection',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();const asset=f.card('source');let n=0;
 f.override('/api/asset',q=>++n===1?pending.promise:{...f.reply('/api/asset',q),fields:[{section:'source',label:'Marker',value:'New response'}]});
 const old=f.context.inspect(asset);await f.settle();await f.context.inspect(asset);await f.settle();
 pending.resolve({...f.reply('/api/asset',{project:'atlas',id:'source'}),fields:[{section:'source',label:'Marker',value:'Old response'}]});await old;
 assert.match(f.text(),/New response/);assert.doesNotMatch(f.text(),/Old response/);
});

test('an older same-record error cannot replace a newer successful inspection',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();const asset=f.card('source');let n=0;
 f.override('/api/asset',q=>++n===1?pending.promise:f.reply('/api/asset',q));
 const old=f.context.inspect(asset);await f.settle();await f.context.inspect(asset);await f.settle();pending.reject(Error('Old inspection failed'));await old;
 assert.match(f.text(),/Source & identity/);assert.doesNotMatch(f.text(),/Old inspection failed/);
});

test('a late record error cannot overwrite a different project inspector',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/asset',q=>q.project==='atlas'?pending.promise:f.reply('/api/asset',q));
 const old=f.context.inspect(f.card('source'));await f.settle();await f.context.inspect(f.card('source','boreal'));
 pending.reject(Error('Old Atlas failure'));await old;assert.equal(f.state().selected.project,'boreal');
 assert.match(f.text(),new RegExp('b'.repeat(40)));assert.doesNotMatch(f.text(),/Old Atlas failure/);
});

test('a pending or failed record load removes previous tabs and actions',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();const asset=f.card('source');await f.context.inspect(asset,'Compare');
 assert.ok(f.button('Use this record as baseline'));f.override('/api/asset',()=>pending.promise);
 const load=f.context.inspect(asset);await f.settle();assert.equal(f.get('tabs').children.length,0);assert.equal(asset.details,undefined);
 assert.match(f.text(),/Reading this exact asset revision/);pending.reject(Error('Inspection failed'));await load;
 assert.equal(f.get('tabs').children.length,0);assert.equal(asset.details,undefined);assert.match(f.text(),/Inspection failed/);
});

test('record inspection refuses mismatched project snapshot and logical identities',async()=>{
 for(const change of [d=>{d.commit='f'.repeat(40);},d=>{d.record.id='other';},d=>{d.card.id='other';},d=>{d.card.project='boreal';}]){
  const f=catalogueFixture();await f.settle();f.override('/api/asset',q=>{const d=f.reply('/api/asset',q);change(d);return d;});
  await f.context.inspect(f.card('source'));assert.match(f.text(),/Record source changed/);assert.equal(f.get('tabs').children.length,0);
  assert.equal(f.state().selected.details,undefined);
 }
});

test('an enrolled snapshot change during inspection refuses the older response',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/asset',()=>pending.promise);
 const load=f.context.inspect(f.card('source'));await f.settle();f.state().projects.find(p=>p.id==='atlas').commit='f'.repeat(40);
 pending.resolve(f.reply('/api/asset',{project:'atlas',id:'source'}));await load;assert.match(f.text(),/Record source changed/);
 assert.equal(f.get('tabs').children.length,0);
});

test('dependency tab reentry ignores the earlier render response',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();await f.context.inspect(f.card('source'));let n=0;
 f.override('/api/dependencies',()=>++n===1?pending.promise:{supported:true,complete_declared_coverage:true,edges:[],gaps:[]});
 await f.click('Dependencies');await f.click('Overview');await f.click('Dependencies');
 pending.resolve({supported:true,complete_declared_coverage:false,edges:[{kind:'stale relationship',from:{id:'source'},target:{id:'old'}}],gaps:[]});await f.settle();
 assert.match(f.text(),/Declared dependency coverage resolved/);assert.doesNotMatch(f.text(),/stale relationship/);
});

test('Escape fences a pending record response and keeps its details unadopted',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();const asset=f.card('source');f.override('/api/asset',()=>pending.promise);
 const load=f.context.inspect(asset);await f.settle();f.document.dispatchEvent({type:'keydown',key:'Escape'});
 pending.resolve(f.reply('/api/asset',{project:'atlas',id:'source'}));await load;assert.equal(f.state().selected,null);assert.equal(asset.details,undefined);
});

test('shipped page bootstraps into admitted works without media or implicit impact IO',async()=>{
 const f=catalogueFixture();await f.settle();assert.equal(f.state().project,'atlas');assert.equal(f.state().mode,'works');
 assert.match(f.text('result-count'),/^1 accepted sequences$/);assert.equal(f.get('assets').children.length,1);
 assert.equal(calls(f,'/api/impact').length,0);assert.equal(f.seen.every(r=>r.init.headers['X-Git-Asset-Session']==='synthetic-session'),true);
 assert.equal(f.seen.every(r=>r.init.cache==='no-store'&&r.init.credentials==='omit'),true);
});
test('work to source impact uses real inspector wiring and preserves work return',async()=>{
 const f=catalogueFixture();await openImpact(f);assert.equal(f.state().selected.id,'source');assert.equal(f.state().tab,'Impact');
 assert.match(f.text(),/Choose relationships and trace consumers/);assert.equal(calls(f,'/api/impact').length,0);
 assert.equal(f.get('back-work').hidden,false);await f.click(f.get('back-work'));
 assert.equal(f.state().selected.work.id,'sequence-01');assert.equal(f.state().tab,'Overview');
});
test('impact results show exact snapshot, stable consumer IDs and visible progress',async()=>{
 const f=catalogueFixture();await openImpact(f);await f.click('Trace consumers');
 assert.match(f.text(),/Showing 2 of 2 known consumers/);assert.match(f.text(),new RegExp('Snapshot '+'a'.repeat(40)));
 assert.match(f.text(),/render-01/);assert.match(f.text(),/render-02/);
 assert.ok(f.button('Inspect Shared output title (render-01)'));
 assert.equal(f.document.activeElement,f.get('impact-feedback'));
});
test('consumer to impact to work preserves results and original navigation',async()=>{
 const f=catalogueFixture();await openImpact(f);await f.click('Trace consumers');
 await f.click('Inspect Shared output title (render-01)');assert.equal(f.state().selected.id,'render-01');
 assert.equal(f.get('back-work').textContent,'← Back to impact');await f.click(f.get('back-work'));
 assert.equal(f.state().selected.id,'source');assert.equal(f.state().tab,'Impact');assert.match(f.text(),/Showing 2 of 2/);
 assert.equal(calls(f,'/api/impact').length,1);assert.equal(f.get('back-work').textContent,'← Back to work');
 await f.click(f.get('back-work'));assert.equal(f.state().selected.work.id,'sequence-01');
});
test('25-row pages retain scope and basis while reporting 25, 50 and 63 shown',async()=>{
 const f=catalogueFixture({count:63});await openImpact(f);await f.click('Trace consumers');assert.match(f.text(),/Showing 25 of 63/);
 await f.click('Show more consumers');assert.match(f.text(),/Showing 50 of 63/);
 await f.click('Show more consumers');assert.match(f.text(),/Showing 63 of 63/);
 const requests=calls(f,'/api/impact');assert.deepEqual(requests.map(r=>r.q.offset),['0','25','50']);
 assert.equal(requests[1].q.basis,'c'.repeat(64));assert.equal(requests[2].q.basis,'c'.repeat(64));
 assert.equal(f.nodes().some(e=>e.tag==='button'&&e.textContent==='Show more consumers'),false);
 assert.equal(new Set(f.state().selected.impactView.rows.map(r=>r.asset.id)).size,63);
});
test('partial zero is explicitly unknown and missing rule samples disclose their limit',async()=>{
 const f=catalogueFixture({partial:true,ruleCount:128});await openImpact(f);await f.click('Trace consumers');
 assert.match(f.text(),/0 known consumers/);assert.match(f.text(),/Coverage incomplete/);
 assert.match(f.text(),/Showing 100 of 128 missing definition pins/);
 assert.match(f.text(),/No known consumers; unresolved relationships remain/);
});
test('complete zero after explicit kinds change remains scoped and requires a new trace',async()=>{
 const f=catalogueFixture();await openImpact(f);await f.click('Trace consumers');
 await f.click(f.get('impact-composition'));await f.click(f.get('impact-publication'));
 assert.equal(calls(f,'/api/impact').length,1);assert.doesNotMatch(f.text(),/known consumers/);
 await f.click('Trace consumers');assert.match(f.text(),/No declared consumers in this snapshot for the chosen relationship kinds/);
 assert.equal(calls(f,'/api/impact')[1].q.kinds,'production');
});
test('historical pins are visible without a current-record navigation button',async()=>{
 const f=catalogueFixture({historical:true});await openImpact(f);await f.click('Trace consumers');
 assert.match(f.text(),/Historical revision/);assert.equal(f.nodes().some(e=>e.getAttribute('aria-label')==='Inspect Shared output title (render-01)'),false);
 assert.ok(f.button('Inspect Shared output title (render-02)'));
});
test('failed paged basis clears old results, explains recovery and allows an explicit fresh trace',async()=>{
 const f=catalogueFixture({count:26});await openImpact(f);await f.click('Trace consumers');
 f.override('/api/impact',q=>Number(q.offset)>0?{fixtureError:'stale_basis'}:f.reply('/api/impact',q));
 await f.click('Show more consumers');assert.doesNotMatch(f.text(),/Showing 25 of 26/);assert.match(f.text(),/Refresh this project and reopen the asset/);
 await f.click('Trace consumers');assert.match(f.text(),/Showing 25 of 26/);assert.equal(calls(f,'/api/impact').at(-1).q.offset,'0');
});
test('a filter change during an in-flight trace rejects the old response',async()=>{
 const f=catalogueFixture(),pending=deferred();await openImpact(f);let old;
 f.override('/api/impact',q=>{old=q;return pending.promise;});const trace=f.click('Trace consumers');await f.settle();
 await f.click(f.get('impact-evidence'));f.override('/api/impact',q=>f.reply('/api/impact',q));await f.click('Trace consumers');
 const current=f.state().selected.impactView.report;pending.resolve({...f.reply('/api/impact',old),status:'partial'});await trace;
 assert.equal(f.state().selected.impactView.report,current);assert.equal(current.status,'complete');
});
test('a late work-inspection error cannot overwrite a newer project work',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/work',q=>q.project==='atlas'?pending.promise:f.reply('/api/work',q));
 const old=f.click('Open Fixture sequence');await f.settle();f.document.dispatchEvent({type:'keydown',key:'Escape'});
 await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));await f.click('Open Fixture sequence');
 pending.reject(Error('Old Atlas work failure'));await old;assert.equal(f.state().selected.work.project,'boreal');
 assert.doesNotMatch(f.text(),/Old Atlas work failure/);assert.match(f.text(),/Current work/);
});
test('a late work-list error cannot mark a newer project view as failed',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/works',q=>q.project==='atlas'?pending.promise:f.reply('/api/works',q));
 const old=f.context.load();await f.settle();await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));
 pending.reject(Error('Old work-list failure'));await old;assert.equal(f.state().project,'boreal');assert.equal(f.get('notice').hidden,true);
 assert.equal(f.get('assets').children.length,1);
});
test('a late search error cannot contaminate a new project selection',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/search',()=>pending.promise);
 f.state().q='old query';f.state().searchScope='all';const old=f.context.load();await f.settle();
 await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));
 pending.reject(Error('Old search failure'));await old;assert.equal(f.get('notice').hidden,true);assert.equal(f.state().project,'boreal');
});
test('a late metadata refresh cannot replace a newer project with an error page',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/projects',()=>pending.promise);
 const old=f.click(f.get('refresh'));await f.settle();await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));
 pending.reject(Error('Old refresh failure'));await old;assert.equal(f.get('notice').hidden,true);assert.equal(f.state().project,'boreal');
 assert.equal(f.get('assets').children[0].getAttribute('aria-label'),'Open Fixture sequence');
});
test('a late successful metadata refresh also leaves a newer project selection intact',async()=>{
 const f=catalogueFixture(),pending=deferred();await f.settle();f.override('/api/projects',()=>pending.promise);
 const old=f.click(f.get('refresh'));await f.settle();await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));
 pending.resolve({...f.reply('/api/projects',{}),projects:[]});await old;
 assert.equal(f.state().project,'boreal');assert.equal(f.state().projects.length,2);assert.equal(f.get('assets').children.length,1);
});
test('summary filters combine actual work requests and keep internal record counts separate',async()=>{
 const f=catalogueFixture();await f.settle();const totals=f.nodes().filter(e=>e.classList.contains('summary-filter'));
 await f.click(totals[1],{ctrlKey:true});assert.equal(calls(f,'/api/works').at(-1).q.collections,'sequences,boards');
 assert.equal(f.get('assets').children.length,2);assert.match(f.get('technical').textContent,/4/);
 assert.equal(f.state().workCollections.length,2);
});
