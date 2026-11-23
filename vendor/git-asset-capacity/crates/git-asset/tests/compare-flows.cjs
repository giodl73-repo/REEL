'use strict';
const {test}=require('node:test'),assert=require('node:assert/strict');
const {catalogueFixture,deferred}=require('./ux/catalogue-fixture.cjs');
async function openComparison(f){await f.settle();await f.click('Open Fixture sequence');await f.click('Compare source record');}
async function baselineThenOtherProject(f){await openComparison(f);await f.click('Use this record as baseline');await f.click(f.get('close'));await f.click(f.get('projects').children.find(e=>e.textContent.includes('Boreal')));await f.click('Open Fixture sequence');await f.click('Compare source record');}
const calls=f=>f.seen.filter(r=>r.path==='/api/compare');

test('Compare opens explicitly with no baseline and performs no comparison IO',async()=>{
 const f=catalogueFixture();await openComparison(f);assert.equal(f.state().tab,'Compare');
 assert.match(f.text(),/No baseline selected/);assert.equal(f.button('Compare with baseline').disabled,true);assert.equal(calls(f).length,0);
 await f.click('Use this record as baseline');assert.equal(f.document.activeElement,f.button('Compare with baseline'));
 await f.click('Clear baseline');assert.equal(f.document.activeElement,f.button('Use this record as baseline'));assert.equal(calls(f).length,0);
});
test('baseline survives work and project navigation with its exact source pin',async()=>{
 const f=catalogueFixture({selectedBytes:true});await baselineThenOtherProject(f);
 assert.match(f.text(),/Baseline recordProjectAtlas/);assert.match(f.text(),/Current recordProjectBoreal/);
 assert.match(f.text(),new RegExp('Snapshot'+'a'.repeat(40)));assert.match(f.text(),new RegExp('Snapshot'+'b'.repeat(40)));
 assert.equal(calls(f).length,0);assert.equal(f.get('back-work').textContent,'← Back to work');
});
test('same selected bytes are explained separately from different records and changed field names',async()=>{
 const f=catalogueFixture({selectedBytes:true});await baselineThenOtherProject(f);await f.click('Compare with baseline');
 assert.match(f.text(),/Selected bytesSame registered SHA-256 and length/);assert.match(f.text(),/Record bytesDifferent/);
 assert.match(f.text(),/Changed authored field nameslanguage/);assert.match(f.text(),/Values are omitted/);
 assert.match(f.text(),/Equal selected bytes can belong to different records or recipes/);
 assert.equal(f.document.activeElement,f.get('comparison-feedback'));
 const q=calls(f)[0].q;assert.equal(q.left_project,'atlas');assert.equal(q.right_project,'boreal');assert.equal(q.left_id,'source');assert.equal(q.right_id,'source');
 assert.equal(q.left_snapshot,'a'.repeat(40));assert.equal(q.right_snapshot,'b'.repeat(40));
});
test('self-comparison preserves unknown selected bytes when a planned record has no selection',async()=>{
 const f=catalogueFixture();await openComparison(f);await f.click('Use this record as baseline');await f.click('Compare with baseline');
 assert.match(f.text(),/Unavailable · one or both records have no selected bytes/);assert.match(f.text(),/Record bytesSame/);
 assert.match(f.text(),/No authored field differences/);assert.doesNotMatch(f.text(),/Selected bytesSame/);
});
test('definition and selected-byte differences remain separate comparison facts',async()=>{
 const f=catalogueFixture({selectedBytes:true});await baselineThenOtherProject(f);
 f.override('/api/compare',q=>{const r=f.reply('/api/compare',q);r.data.definition_equal=false;r.data.selected_equal=false;return r;});
 await f.click('Compare with baseline');assert.match(f.text(),/DefinitionDifferent/);assert.match(f.text(),/Selected bytesDifferent registered/);
});
test('stream additions, removals and role/group changes show a clear baseline-to-current direction',async()=>{
 const f=catalogueFixture({selectedBytes:true});await baselineThenOtherProject(f);
 f.override('/api/compare',q=>{const r=f.reply('/api/compare',q),object=f.reference('stream').object;
  const before={slot:'shared',role:'input',group:'old',object},after={slot:'shared',role:'result',group:'new',object};
  r.data.streams={added:[{...after,slot:'new'}],removed:[{...before,slot:'old'}],changed:[{slot:'shared',before,after}],unchanged:2};return r;});
 await f.click('Compare with baseline');assert.match(f.text(),/Named streams · baseline → current/);
 assert.match(f.text(),/1 added · 1 removed · 1 changed · 2 unchanged/);assert.match(f.text(),/Added · new/);assert.match(f.text(),/Removed · old/);
 assert.match(f.text(),/Changed slot · shared/);assert.match(f.text(),/Before · sharedinput · old/);assert.match(f.text(),/After · sharedresult · new/);
});
test('changing the baseline invalidates an in-flight comparison and requires an explicit new query',async()=>{
 const f=catalogueFixture({selectedBytes:true}),pending=deferred();await baselineThenOtherProject(f);let q;
 f.override('/api/compare',query=>{q=query;return pending.promise;});const old=f.click('Compare with baseline');await f.settle();
 await f.click('Use this record as baseline');assert.doesNotMatch(f.text(),/Comparison result/);
 pending.resolve(f.reply('/api/compare',q));await old;assert.doesNotMatch(f.text(),/Comparison result/);assert.equal(calls(f).length,1);
 f.override('/api/compare',query=>f.reply('/api/compare',query));await f.click('Compare with baseline');assert.match(f.text(),/Record bytesSame/);
 assert.equal(calls(f)[1].q.left_project,'boreal');
});
test('clearing the baseline cancels pending results and disables comparison without IO',async()=>{
 const f=catalogueFixture(),pending=deferred();await baselineThenOtherProject(f);let q;
 f.override('/api/compare',query=>{q=query;return pending.promise;});const old=f.click('Compare with baseline');await f.settle();
 await f.click('Clear baseline');pending.resolve(f.reply('/api/compare',q));await old;
 assert.match(f.text(),/No baseline selected/);assert.equal(f.button('Compare with baseline').disabled,true);assert.doesNotMatch(f.text(),/Comparison result/);
});
test('a comparison reply arriving after closing the inspector cannot reopen it',async()=>{
 const f=catalogueFixture(),pending=deferred();await baselineThenOtherProject(f);let q;
 f.override('/api/compare',query=>{q=query;return pending.promise;});const old=f.click('Compare with baseline');await f.settle();
 await f.click(f.get('close'));pending.resolve(f.reply('/api/compare',q));await old;assert.equal(f.get('inspector').hidden,true);assert.equal(f.state().selected,null);
});
test('changed left/right record pins, selectors or snapshots refuse mixed comparison results',async()=>{
 for(const change of ['left-pin','right-pin','selector','snapshot']){
  const f=catalogueFixture({selectedBytes:true});await baselineThenOtherProject(f);
  f.override('/api/compare',q=>{const r=f.reply('/api/compare',q);if(change==='left-pin')r.data.left.record.object.sha256='0'.repeat(64);if(change==='right-pin')r.data.right.record.object.bytes++;if(change==='selector')r.data.left.selector.project='boreal';if(change==='snapshot')r.snapshots.atlas='0'.repeat(40);return r;});
  await f.click('Compare with baseline');assert.match(f.text(),/Comparison source changed/);assert.doesNotMatch(f.text(),/Comparison result/);
 }
});
test('a refused comparison explains recovery and supports an explicit retry',async()=>{
 const f=catalogueFixture();await baselineThenOtherProject(f);f.override('/api/compare',()=>({fixtureError:'Snapshot changed; refresh the selected project'}));
 await f.click('Compare with baseline');assert.match(f.text(),/Refresh the affected project and choose the baseline again/);
 f.override('/api/compare',q=>f.reply('/api/compare',q));await f.click('Compare with baseline');assert.match(f.text(),/Comparison result/);assert.equal(calls(f).length,2);
});
test('authored names and slot labels remain text and raw authored values are never requested',async()=>{
 const f=catalogueFixture();await baselineThenOtherProject(f);
 f.override('/api/compare',q=>{const r=f.reply('/api/compare',q);r.data.authored_changed_fields=['<img src=x onerror=alert(1)>'];return r;});
 await f.click('Compare with baseline');assert.match(f.text(),/<img src=x/);assert.equal(calls(f).length,1);assert.equal(Object.hasOwn(calls(f)[0].q,'authored'),false);
});
