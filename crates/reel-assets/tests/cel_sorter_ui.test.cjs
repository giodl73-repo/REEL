const test=require('node:test');const assert=require('node:assert/strict');const ui=require('../src/cel_sorter.js');
const rows=[{semantic_id:'a',picture_binding:'one',frames:41},{semantic_id:'b',picture_binding:'two',frames:59}];
test('native recordings preserve independent cue clocks and exact seek boundaries',()=>{
 const recordings=[{cue_id:'one',sha256:'a'.repeat(64),src:'media/'+'a'.repeat(64)+'.wav',bytes:100,samples:96,sample_rate:48,start_seconds:0},{cue_id:'two',sha256:'b'.repeat(64),src:'media/'+'b'.repeat(64)+'.wav',bytes:120,samples:88,sample_rate:44,start_seconds:2}];
 const session={media_mode:'native-recordings',movie:null,recordings,cues:recordings.map(({cue_id,samples,sample_rate,start_seconds})=>({cue_id,samples,sample_rate,start_seconds}))};
 const actual=ui.recordings(session);assert.deepEqual(ui.recordingAt(actual,1.5),{index:0,local:1.5});assert.deepEqual(ui.recordingAt(actual,2),{index:1,local:0});assert.deepEqual(ui.recordingAt(actual,3.5),{index:1,local:1.5});assert.deepEqual(ui.recordingAt(actual,99),{index:1,local:2});
 for(const mutate of [s=>s.movie='performance.mp4',s=>s.recordings.pop(),s=>s.recordings[1].cue_id='one',s=>s.recordings[1].start_seconds=1.9,s=>s.recordings[1].sample_rate=48,s=>s.recordings[1].src='../private.wav',s=>s.recordings[1].samples=0,s=>s.recordings[1].bytes=-1]){const bad=structuredClone(session);mutate(bad);assert.throws(()=>ui.recordings(bad));}
 assert.equal(ui.recordings({media_mode:'controls-only'}),null);assert.equal(ui.recordings({movie:'performance.mp4'}),null);
 assert.equal(ui.recordings({media_mode:'native-recordings',sample_rate:48,end_sample:96,recordings:[recordings[0]]}).length,1);
});
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

const cues=[{semantic_id:'a',picture_binding:'one',cue_id:'first',samples:47,sample_rate:48},{semantic_id:'b',picture_binding:'two',cue_id:'first',samples:49,sample_rate:48},{semantic_id:'c',picture_binding:'one',cue_id:'second',samples:43,sample_rate:44},{semantic_id:'d',picture_binding:'two',cue_id:'second',samples:45,sample_rate:44}];
test('sample cue groups fit independently with exact budgets and retain comments',()=>{
 const edited=[{...cues[1],samples:20,comment:'Keep this cut'},cues[0],cues[3]];
 assert(ui.valid(edited,cues,null));assert(!ui.valid(edited,cues,4));
 const fit=ui.fit(edited,4,cues);assert(ui.valid(fit,cues,4));
 assert.equal(fit[0].samples+fit[1].samples,96);assert.equal(fit[2].samples,88);
 assert.equal(fit[0].comment,'Keep this cut');assert.equal(fit[0].semantic_id,'b');
 assert.equal(cues[0].samples,47);
});
test('cross cue edits, altered rates and removing whole cues reject',()=>{
 for(const rows of [[cues[2],cues[0],cues[1],cues[3]],[{...cues[0],cue_id:'second'},...cues.slice(1)],[{...cues[0],sample_rate:44},...cues.slice(1)],cues.slice(0,2),[{...cues[0],frames:10},...cues.slice(1)]])assert(!ui.valid(rows,cues,null));
 assert.throws(()=>ui.fit(cues.slice(0,2),4,cues));
});
test('equal total duration cannot hide drift between native cues',()=>{
 const edited=structuredClone(cues);edited[0].samples+=48;edited[2].samples-=22;edited[3].samples-=22;
 assert.equal(edited.reduce((s,r)=>s+ui.duration(r,24),0),4);
 assert(!ui.valid(edited,cues,4));
});
test('preview uses seconds and fixes the next cue boundary during temporary hold edits',()=>{
 const edited=structuredClone(cues);edited[0].samples+=48;
 const starts=ui.timeStarts(edited,cues,24);assert.equal(starts[0],0);assert.equal(starts[1],95/48);
 assert.equal(starts[2],2);assert.equal(starts[3],2+43/44);
 assert.deepEqual(ui.timeStarts(rows,rows,10),[0,4.1]);
});
test('v3 drafts cannot masquerade as frame edits and preserve project locks',()=>{
 const session={schema:'reel.cel-sorter-session.v3',scene_id:'synthetic',expected_project:'a'.repeat(64),expected_inputs:{'alignment:first':'first','alignment:second':'second'}};
 const request=ui.draft(session,cues,'Keep cues');assert.equal(request.schema,'reel.cel-sorter-edit.v3');assert(ui.matches(session,request));
 const frame={...session,schema:'reel.cel-sorter-session.v2'};assert(!ui.matches(frame,request));assert.notEqual(ui.key(frame),ui.key(session));
 assert(!ui.valid([{...rows[0],samples:1},rows[1]],rows,100));
});

test('delivery rate observations warn about mismatches without claiming build acceptance',()=>{
 assert.match(ui.clockNotice({delivery_clock:{state:'mixed-native-rates'}}),/different sample rates/);
 assert.match(ui.clockNotice({delivery_clock:{state:'profile-rate-mismatch'}}),/before rendering/);
 assert.equal(ui.clockNotice({delivery_clock:{state:'matches-profile'}}),'');
 assert.equal(ui.clockNotice({delivery_clock:{state:'not-declared'}}),'');assert.equal(ui.clockNotice({}),'');
});

test('scoped v4 requests bind language and isolate older autosaves',()=>{
 const session={schema:'reel.cel-sorter-session.v4',language:'es',scene_id:'synthetic',expected_project:'a'.repeat(64),expected_inputs:{scene:'source'}};
 const request=ui.draft(session,cues,'Scoped note');assert.equal(request.schema,'reel.cel-sorter-edit.v4');assert.equal(request.language,'es');assert(ui.matches(session,request));
 assert(!ui.matches(session,{...request,language:'en'}));assert(!ui.matches(session,{...request,language:undefined}));assert(!ui.matches(session,{...request,schema:'reel.cel-sorter-edit.v3'}));
 const old={...session,schema:'reel.cel-sorter-session.v3'};delete old.language;assert.notEqual(ui.key(session),ui.key(old));assert(!ui.matches(old,request));
 for(const language of [undefined,'',' ','x'.repeat(65)])assert.throws(()=>ui.context({...session,language}));
 assert.throws(()=>ui.context({...old,language:'es'}));
});

test('explicit picture choices permit substitution without admitting new cel identities',()=>{
 const pictures=['one','two','candidate'];
 const changed=[{...rows[0],picture_binding:'candidate',comment:'Keep the new picture'},...rows.slice(1)];
 assert(!ui.valid(changed,rows,null));assert(ui.valid(changed,rows,null,pictures));
 const fitted=ui.fit(changed,100,rows,pictures);assert(ui.valid(fitted,rows,100,pictures));
 assert.equal(fitted[0].comment,'Keep the new picture');
 assert(!ui.valid([{...changed[0],semantic_id:'new-cel'},...changed.slice(1)],rows,null,pictures));
 assert(!ui.valid([{...changed[0],picture_binding:'unlisted'},...changed.slice(1)],rows,null,pictures));
});
