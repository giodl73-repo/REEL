const test=require('node:test');const assert=require('node:assert/strict');const ui=require('../src/cel_sorter.js');
const rows=[{semantic_id:'a',picture_binding:'one',frames:41},{semantic_id:'b',picture_binding:'two',frames:59}];
test('frame starts follow reordered holds',()=>assert.deepEqual(ui.starts([...rows].reverse()),[0,59]));
test('fit is exact, positive and preserves stable identities',()=>{const fit=ui.fit(rows,123);assert.equal(fit.reduce((s,r)=>s+r.frames,0),123);assert.equal(fit[0].semantic_id,'a');assert(fit.every(r=>r.frames>0));assert.equal(rows[0].frames,41);});
test('reject duplicates, missing coverage and foreign images',()=>{assert(ui.valid(rows,rows,100));assert(!ui.valid([rows[0],rows[0]],rows,82));assert(!ui.valid(rows,rows,99));assert(!ui.valid([{...rows[0],picture_binding:'foreign',frames:100}],rows,100));});
test('removal can fit remaining image to full recording',()=>{const fit=ui.fit([rows[0]],100);assert(ui.valid(fit,rows,100));});
test('cel comments survive reorder, hold fitting and saved draft roundtrip',()=>{
 const commented=rows.map((r,i)=>({...r,comment:i===0?'Fix the hand.\nKeep the warm light.':'Longer hold — lovely scene.'}));
 const edited=ui.fit([...commented].reverse(),100);
 const session={schema:'reel.cel-sorter-session.v1',scene_id:'synthetic',expected_inputs:{'scene.json':'source-lock'}};
 const draft=ui.draft(session,edited,'Overall note');
 const reopened=JSON.parse(JSON.stringify(draft));
 assert.equal(reopened.rows[0].semantic_id,'b');
 assert.equal(reopened.rows[0].comment,'Longer hold — lovely scene.');
 assert.equal(reopened.rows[1].comment,'Fix the hand.\nKeep the warm light.');
 assert.equal(reopened.notes,'Overall note');assert(ui.valid(reopened.rows,rows,100));
 edited[0].comment='Changed later';assert.equal(draft.rows[0].comment,'Longer hold — lovely scene.');
 assert.equal(session.expected_inputs['scene.json'],'source-lock');
});
test('legacy drafts without comments remain valid; nontext comments fail',()=>{
 assert(ui.valid(JSON.parse(JSON.stringify(rows)),rows,100));
 assert(!ui.valid([{...rows[0],comment:{unexpected:'object'}},rows[1]],rows,100));
});
test('mapped projects roundtrip their exact descriptor lock and cel comments',()=>{
 const session={schema:'reel.cel-sorter-session.v2',expected_project:'a'.repeat(64),scene_id:'synthetic',expected_inputs:{scene:'x',bindings:'y'}};
 const draft=ui.draft(session,[{...rows[0],comment:'Keep this note'}],'Review');
 assert.equal(draft.schema,'reel.cel-sorter-edit.v2');assert.equal(draft.expected_project,session.expected_project);
 assert(ui.matches(session,JSON.parse(JSON.stringify(draft))));
 assert.equal(draft.rows[0].comment,'Keep this note');
 assert(!ui.matches({...session,expected_project:'b'.repeat(64)},draft));
 assert(!ui.matches(session,{...draft,schema:'reel.cel-sorter-edit.v1',expected_project:undefined}));
 assert(!ui.matches(session,{...draft,expected_inputs:{scene:'changed',bindings:'y'}}));
});
test('legacy browser keys stay compatible and mapped identities remain separate',()=>{
 const legacy={schema:'reel.cel-sorter-session.v1',scene_id:'synthetic',expected_inputs:{scene:'x'}};
 assert.equal(ui.key(legacy),'reel-cel-sorter:'+JSON.stringify(legacy.expected_inputs));
 const mapped={...legacy,schema:'reel.cel-sorter-session.v2',expected_project:'a'.repeat(64)};
 assert.notEqual(ui.key(mapped),ui.key(legacy));
 assert.notEqual(ui.key(mapped),ui.key({...mapped,expected_project:'b'.repeat(64)}));
 assert(!ui.matches(legacy,ui.draft(mapped,rows,'')));
 assert.throws(()=>ui.draft({...legacy,schema:'future'},rows,''));
 assert.throws(()=>ui.draft({...mapped,expected_project:undefined},rows,''));
});
test('source lock maps compare by entries without ignoring extra or changed keys',()=>{
 assert(ui.locksEqual({scene:'a',bindings:'b'},{bindings:'b',scene:'a'}));
 assert(!ui.locksEqual({scene:'a'},{scene:'a',extra:'b'}));
 assert(!ui.locksEqual({scene:'a'},{scene:'changed'}));
 assert.equal(ui.locksEqual(null,{}),false);assert.equal(ui.locksEqual([],[]),false);
});
