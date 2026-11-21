'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm');
const {catalogueFixture,deferred}=require('./ux/catalogue-fixture.cjs');
const owner=(action,project='Atlas',slot='selected')=>action+' · '+project+' · source · '+slot;
async function objects(f){await f.settle();await f.click('Open Fixture sequence');await f.click('Manage this work’s objects');}
async function object(f){await objects(f);await f.click('Shared fixture bytes');}
async function references(f){await object(f);await f.click('References & versions');}
const requests=(f,path)=>f.seen.filter(r=>r.path===path);

test('work objects count content once and copies separately without payload or refresh requests',async()=>{
 const f=catalogueFixture({objectMode:true});await objects(f);
 assert.equal(f.state().mode,'objects');assert.match(f.nodes().map(e=>e.textContent).join('\n'),/1matching byte objects/);
 const data=f.reply('/api/storage/objects',{});assert.equal(data.stored_file_bytes,data.unique_content_bytes*2);
 assert.equal(requests(f,'/api/storage/objects')[0].q.work,'sequence-01');
 assert.equal(f.seen.some(r=>/verify|observe|expand|preview/.test(r.path)),false);
});
test('object references expose explicit owner shortcuts and impact returns through its consumer to the same tab',async()=>{
 const f=catalogueFixture({objectMode:true});await references(f);
 assert.match(f.text(),/Retained or nested bindings do not prove current production use/);
 await f.click(owner('Trace owner record impact'));assert.equal(f.state().selected.id,'source');assert.equal(f.state().tab,'Impact');
 await f.click('Trace consumers');await f.click('Inspect Shared output title (render-01)');await f.click(f.get('back-work'));
 assert.equal(f.state().tab,'Impact');await f.click(f.get('back-work'));
 assert.equal(f.state().tab,'References & versions');assert.ok(f.button(owner('Compare owner record')));
 assert.equal(f.seen.some(r=>/verify|observe|expand|preview/.test(r.path)),false);
});
test('two declaring records can be compared across projects while retaining object navigation',async()=>{
 const f=catalogueFixture({objectMode:true});await references(f);await f.click(owner('Compare owner record'));
 await f.click('Use this record as baseline');await f.click(f.get('back-work'));
 await f.click(owner('Compare owner record','Boreal','history/v1'));await f.click('Compare with baseline');
 assert.match(f.text(),/Selected bytesSame registered/);assert.match(f.text(),/Record bytesDifferent/);
 assert.match(f.text(),/1 added · 0 removed/);const q=requests(f,'/api/compare')[0].q;
 assert.equal(q.left_project,'atlas');assert.equal(q.right_project,'boreal');assert.equal(q.left_id,'source');
 await f.click(f.get('back-work'));assert.equal(f.state().tab,'References & versions');
 assert.match(f.text(),/Other declared revision/);
});
test('nested provenance and binding samples remain explicit without expanding any metadata',async()=>{
 const f=catalogueFixture({objectMode:true}),document=f.reference('manifest').object;
 f.override('/api/storage/objects',q=>{const d=f.reply('/api/storage/objects',q);if(q.sha){d.items[0].binding_count=125;d.items[0].bindings[0].nested={adapter:'fixture',document,pointer:'/assets/0'};}return d;});
 await references(f);assert.match(f.text(),/Showing 3 of 125 registered bindings/);assert.match(f.text(),new RegExp('document '+document.sha256));
 assert.match(f.text(),/\/assets\/0/);assert.equal(f.seen.some(r=>/expand|observe|verify/.test(r.path)),false);
 const label=owner('Open owner record')+' · '+document.sha256.slice(0,12)+' · /assets/0';await f.click(label);
 assert.equal(f.state().selected.id,'source');assert.equal(f.state().selected.project,'atlas');
});
test('a pending object request clears old record content and actions',async()=>{
 const f=catalogueFixture({objectMode:true}),pending=deferred();await objects(f);let q;
 f.override('/api/storage/objects',query=>{q=query;return pending.promise;});const open=f.click('Shared fixture bytes');await f.settle();
 assert.match(f.text(),/Loading registered object metadata/);assert.equal(f.get('tabs').children.length,0);assert.doesNotMatch(f.text(),/Compare source record/);
 pending.resolve(f.reply('/api/storage/objects',q));await open;assert.match(f.text(),/This object/);
});
test('missing objects show a finite scoped error and explicit reload can recover',async()=>{
 const f=catalogueFixture({objectMode:true});await objects(f);
 f.override('/api/storage/objects',q=>({...f.reply('/api/storage/objects',q),items:[],total:0}));await f.click('Shared fixture bytes');
 assert.match(f.text(),/Object not found in the registered scope/);assert.equal(f.get('tabs').children.length,0);
 f.override('/api/storage/objects',q=>f.reply('/api/storage/objects',q));await f.click('Reload object');assert.match(f.text(),/Unique content/);
});
test('wrong object hashes or lengths cannot replace the requested identity',async()=>{
 for(const field of ['sha256','bytes']){
  const f=catalogueFixture({objectMode:true});await objects(f);f.override('/api/storage/objects',q=>{const d=f.reply('/api/storage/objects',q);if(field==='bytes')d.items[0].bytes++;else d.items[0].sha256='0'.repeat(64);return d;});
  await f.click('Shared fixture bytes');assert.match(f.text(),/Object identity changed/);assert.equal(f.get('tabs').children.length,0);
 }
});
test('two opens of the same object use distinct requests so old observations cannot overwrite newer ones',async()=>{
 const f=catalogueFixture({objectMode:true}),pending=deferred();await objects(f);let q,n=0;
 f.override('/api/storage/objects',query=>{if(n++===0){q=query;return pending.promise;}const d=f.reply('/api/storage/objects',query);d.items[0].basis='f'.repeat(64);return d;});
 const old=f.click('Shared fixture bytes');await f.settle();f.document.dispatchEvent({type:'keydown',key:'Escape'});
 await f.click('Shared fixture bytes');assert.equal(f.state().selected.fullObject.basis,'f'.repeat(64));
 pending.resolve(f.reply('/api/storage/objects',q));await old;assert.equal(f.state().selected.fullObject.basis,'f'.repeat(64));
});
test('an old object error cannot overwrite a newer record inspector',async()=>{
 const f=catalogueFixture({objectMode:true}),pending=deferred();await objects(f);f.override('/api/storage/objects',()=>pending.promise);
 const old=f.click('Shared fixture bytes');await f.settle();f.document.dispatchEvent({type:'keydown',key:'Escape'});
 await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));await f.click('Open Fixture sequence');await f.click('Compare source record');
 pending.reject(Error('Old object failure'));await old;assert.equal(f.state().tab,'Compare');assert.match(f.text(),/Current recordProjectBoreal/);assert.doesNotMatch(f.text(),/Old object failure/);
});
test('owner navigation refuses when the captured project snapshot is stale',async()=>{
 const f=catalogueFixture({objectMode:true});await references(f);f.state().projects.find(p=>p.id==='atlas').commit='0'.repeat(40);
 await f.click(owner('Open owner record'));assert.match(f.text(),/owner project snapshot changed or is unavailable/);
 assert.equal(requests(f,'/api/asset').length,0);assert.equal(f.document.activeElement,f.get('object-link-error'));
});
test('mismatched owner responses clear record actions while preserving back to the object',async()=>{
 const f=catalogueFixture({objectMode:true});await references(f);
 f.override('/api/asset',q=>({...f.reply('/api/asset',q),commit:'0'.repeat(40)}));await f.click(owner('Compare owner record'));
 assert.match(f.text(),/Record source changed/);assert.equal(f.get('tabs').children.length,0);assert.equal(f.get('back-work').hidden,false);
});
test('changed object source snapshots refuse before displaying placement or references',async()=>{
 const f=catalogueFixture({objectMode:true});await objects(f);f.override('/api/storage/objects',q=>({...f.reply('/api/storage/objects',q),snapshots:{atlas:'0'.repeat(40)}}));
 await f.click('Shared fixture bytes');assert.match(f.text(),/project snapshot changed/);assert.equal(f.get('tabs').children.length,0);
});
test('a failed held review cannot add an error to another object tab',async()=>{
 const f=catalogueFixture({objectMode:true}),pending=deferred();await object(f);await f.click('Review actions');f.override('/api/storage/preview',()=>pending.promise);
 const old=f.click('Preview alignment');await f.settle();await f.click('Placement');pending.reject(Error('Old preview failure'));await old;
 assert.doesNotMatch(f.text(),/Old preview failure/);assert.match(f.text(),/Unique content/);
});
test('a successful held review cannot append its proposals to another object tab',async()=>{
 const f=catalogueFixture({objectMode:true}),pending=deferred();await object(f);await f.click('Review actions');f.override('/api/storage/preview',()=>pending.promise);
 const old=f.click('Preview alignment');await f.settle();await f.click('Placement');pending.resolve({caches:[],proposals:[],steps:[],holds:['OLD REVIEW']});await old;
 assert.doesNotMatch(f.text(),/OLD REVIEW|Preview alignment · held/);
});
test('object return and held reviews retain the captured scope instead of a later global filter',async()=>{
 const f=catalogueFixture({objectMode:true});await references(f);await f.click(owner('Open owner record'));
 vm.runInContext("managedObjects.project='boreal'",f.context);await f.click(f.get('back-work'));
 assert.equal(requests(f,'/api/storage/objects').at(-1).q.project,'atlas');assert.equal(f.state().tab,'References & versions');
 await f.click('Review actions');f.override('/api/storage/preview',()=>({caches:[],proposals:[],steps:[],holds:['Fixture hold']}));await f.click('Preview alignment');
 assert.equal(requests(f,'/api/storage/preview')[0].q.project,'atlas');assert.match(f.text(),/Verified reclaimable space: unknown/);
});
test('an observed object without registered declaring records remains explicitly unknown',async()=>{
 const f=catalogueFixture({objectMode:true});f.override('/api/storage/objects',q=>{const d=f.reply('/api/storage/objects',q);d.items[0].bindings=[];d.items[0].binding_count=0;return d;});
 await references(f);assert.match(f.text(),/No declaring record is registered in this view/);assert.match(f.text(),/Unindexed or opaque consumers may still exist/);
 assert.equal(f.nodes().some(e=>e.textContent==='Compare owner record'),false);
});
