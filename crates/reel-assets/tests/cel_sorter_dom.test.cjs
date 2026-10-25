const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
test('primary recording UI handles pending seeks, cue transitions and blocked playback',async()=>{
 class Element {
  constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.dataset={};this.value='';this.classList={toggle(){},add(){},remove(){}};this.paused=true;this.readyState=1;this.currentTime=0;this.duration=2;this.playCalls=0;}
  append(...items){this.children.push(...items);}replaceChildren(...items){this.children=items;}focus(){this.focused=true;}
  addEventListener(name,fn,opts={}){(this.listeners[name]??=[]).push({fn,once:opts.once});}
  emit(name){const events=this.listeners[name]||[];this.listeners[name]=events.filter(e=>!e.once);for(const e of events)e.fn({target:this});}
  setAttribute(name,value){this[name]=value;}click(){this.emit('click');this.onclick?.();}
  pause(){this.paused=true;}play(){this.playCalls++;this.paused=false;return this.block?Promise.reject(Error('blocked')):Promise.resolve();}load(){this.readyState=0;this.currentTime=0;}
 }
 const elements=new Map(),element=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};
 const document={getElementById:element,createElement:tag=>new Element(tag),querySelectorAll:()=>element('filmstrip').children};
 const rows=[{semantic_id:'a',picture_binding:'one',cue_id:'first',samples:96,sample_rate:48},{semantic_id:'b',picture_binding:'two',cue_id:'second',samples:88,sample_rate:44}];
 const recordings=rows.map((r,i)=>({cue_id:r.cue_id,samples:r.samples,sample_rate:r.sample_rate,start_seconds:i*2,sha256:String(i+1).repeat(64),src:'media/'+String(i+1).repeat(64)+'.wav',bytes:100}));
 const session={schema:'reel.cel-sorter-session.v4',language:'es',expected_project:'a'.repeat(64),expected_inputs:{scene:'locked'},scene_id:'test',rows,images:{one:'one.png',two:'two.png'},cues:recordings,recordings,media_mode:'native-recordings',movie:null,total_seconds:4,fps_numerator:24,fps_denominator:1};
 let tick;
 vm.runInNewContext(fs.readFileSync(require.resolve('../src/cel_sorter.js'),'utf8'),{document,fetch:async()=>({ok:true,json:async()=>session}),structuredClone,Blob,TextEncoder,URL,localStorage:{getItem(){},setItem(){}},requestAnimationFrame:fn=>tick=fn,setTimeout,confirm:()=>true});
 await new Promise(setImmediate);const a=element('audio'),slider=element('seek');assert.equal(a.src,recordings[0].src);assert.match(element('media-notice').textContent,/Primary recordings/);
 // Rapid seeks while the next cue is loading must apply only the newest target.
 slider.value=3;slider.oninput();assert.equal(a.src,recordings[1].src);assert.equal(a.readyState,0);slider.value=3.5;slider.oninput();a.readyState=1;a.emit('loadedmetadata');assert.equal(a.currentTime,1.5);tick();assert.match(element('preview').alt,/b/);assert.equal(slider.value,3.5);
 // A paused seek stays paused. Returning to cue one and ending playback advances.
 assert.equal(a.playCalls,0);slider.value=0;slider.oninput();a.readyState=1;a.emit('loadedmetadata');assert.equal(a.currentTime,0);a.currentTime=2;a.emit('ended');a.readyState=1;a.emit('loadedmetadata');await new Promise(setImmediate);assert.equal(a.src,recordings[1].src);assert.equal(a.currentTime,0);assert.equal(a.playCalls,1);
 // Browsers may block resumed playback; preserve the clock and offer Play.
 a.block=true;slider.value=.5;slider.oninput();a.readyState=1;a.emit('loadedmetadata');await new Promise(setImmediate);assert.match(element('status').textContent,/Press Play/);a.emit('error');assert.match(element('status').textContent,/could not be played/);
});
// Execute the exported editor and its handlers against a minimal synthetic DOM.
// This tests UI wiring; it is not a browser layout, media or visual acceptance test.
for(const controls of [false,true])for(const scoped of [false,true])test('exported multi-cue UI preserves scope and comments '+(scoped?'v4':'v3')+(controls?' controls-only':' media'),async()=>{
 class Element {
  constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.dataset={};this.value='';this.classList={toggle(){},add(){},remove(){}};}
  append(...items){this.children.push(...items);}replaceChildren(...items){this.children=items;}
  addEventListener(name,fn){this.listeners[name]=fn;}setAttribute(name,value){this[name]=value;}
  click(){this.listeners.click?.({target:this});this.onclick?.();}focus(){this.focused=true;}
 }
 const elements=new Map(),element=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};
 const document={getElementById:element,createElement:tag=>new Element(tag),querySelectorAll:()=>element('filmstrip').children};
 const rows=[{semantic_id:'a',picture_binding:'one',cue_id:'first',samples:47,sample_rate:48},{semantic_id:'b',picture_binding:'two',cue_id:'first',samples:49,sample_rate:48},{semantic_id:'c',picture_binding:'one',cue_id:'second',samples:43,sample_rate:44},{semantic_id:'d',picture_binding:'two',cue_id:'second',samples:45,sample_rate:44}];
 const session={picture_choices:['one','two','candidate'],schema:scoped?'reel.cel-sorter-session.v4':'reel.cel-sorter-session.v3',...(scoped?{language:'es'}:{}),expected_project:'a'.repeat(64),expected_inputs:{scene:'synthetic'},scene_id:'synthetic',rows,images:{one:'one.png',two:'two.png',candidate:'candidate.png'},delivery_clock:{state:'mixed-native-rates'},total_seconds:4,fps_numerator:24,fps_denominator:1,movie:'synthetic.mp4'};
 session.insertion={schema:'reel.cel-sorter-insertion-context.v1',language:'es',anchors:rows.map(r=>({semantic_id:r.semantic_id,cue_id:r.cue_id,canonical_cue_id:'canonical-'+r.cue_id}))};
 session.insertion.score_palette={choices:[{role:'theme',asset_binding:'score.theme'}],held_roles:[{role:'pending',reason:'binding unavailable'}]};
 session.insertion.effects={sonic:{choices:[{binding:'sound'}],held:[]},vfx:{choices:[{binding:'motion'}],held:[{binding:'held-fx',reason:'unselected'}]}};
 if(controls){session.media_mode='controls-only';session.images={};session.movie=null;}
 let downloaded,animation;const saved=new Map();
 vm.runInNewContext(fs.readFileSync(require.resolve('../src/cel_sorter.js'),'utf8'),{document,fetch:async()=>({ok:true,json:async()=>session}),structuredClone,Blob,TextEncoder,crypto:{randomUUID:()=>'synthetic-uuid'},URL:{createObjectURL:blob=>{downloaded=blob;return 'blob:synthetic';},revokeObjectURL(){}},localStorage:{getItem:key=>saved.get(key),setItem:(key,value)=>saved.set(key,value)},requestAnimationFrame:fn=>{animation=fn;},setTimeout:fn=>fn(),confirm:()=>true});
 await new Promise(setImmediate);
 const cards=()=>element('filmstrip').children;
 const descendants=node=>[node,...node.children.flatMap(descendants)];
 const button=(card,label)=>descendants(card).find(node=>node.tagName==='BUTTON'&&node.textContent===label);
 assert.equal(cards().length,4);assert.equal(element('save').disabled,false);assert.equal(element('delivery-clock').hidden,false);assert.match(element('delivery-clock').textContent,/different sample rates/);
 assert(descendants(cards()[2]).some(node=>node.textContent?.startsWith('2.00 s')));
 assert.equal(button(cards()[1],'\u2193').disabled,true);assert.equal(button(cards()[2],'\u2191').disabled,true);
 // The actual form downloads a native insertion without mutating known rows.
 element('new-cel').click();assert.equal(element('insertion-panel').open,true);assert.equal(element('insert-split').focused,true);assert.equal(element('insert-split').value,'23');element('insert-picture').value='candidate';element('insert-score').value='silence';element('insert-save').click();const insertion=JSON.parse(await downloaded.text());assert.equal(insertion.schema,'reel.cel-sorter-insert.v1');assert.equal(insertion.split_after_samples,23);assert.equal(insertion.shared_event.picture_binding,'candidate');assert.equal(insertion.language,'es');assert.equal(cards().length,4);assert.equal(element('save').disabled,false);element('insert-split').value='47';element('insert-save').click();assert.match(element('status').textContent,/positive hold/);element('insert-split').value='23';
 element('insert-score').value='role:theme';element('insert-save').click();const scored=JSON.parse(await downloaded.text());assert.equal(scored.shared_event.score.disposition,'role');assert.equal(scored.shared_event.score.role,'theme');assert.match(element('insert-score-notice').textContent,/not mixed.*pending/);
 const effectInputs=id=>descendants(element(id)).filter(n=>n.tagName==='INPUT');const sonic=effectInputs('insert-sonic')[0],fx=effectInputs('insert-vfx')[0];assert.equal(sonic.checked,false);assert.equal(fx.checked,false);sonic.checked=true;sonic.listeners.change();fx.checked=true;fx.listeners.change();element('insert-save').click();const effected=JSON.parse(await downloaded.text());assert.equal(effected.shared_event.sonic_bindings[0],'sound');assert.equal(effected.shared_event.vfx_bindings[0],'motion');assert.equal(cards().length,4);
 // Explicit candidate pictures are selectable in media and controls-only modes.
 const choose=descendants(cards()[1]).find(node=>node.tagName==='SELECT');
 assert.deepEqual(Array.from(choose.children,node=>node.value),['one','two','candidate']);
 choose.value='candidate';choose.listeners.change();assert.equal(element('save').disabled,false);element('insert-save').click();assert.match(element('status').textContent,/Save and apply/);
 // Dragging across cues rolls back the actual UI state.
 cards()[0].listeners.dragstart({target:cards()[0],dataTransfer:{setData(){}},preventDefault(){}});
 cards()[2].listeners.drop({preventDefault(){}});assert.equal(cards().length,4);
 assert(descendants(cards()[0]).some(node=>node.textContent?.includes('first \u00b7 a')));
 // Comments follow the remaining cel through removal, fit and download.
 const comment=descendants(cards()[1]).find(node=>node.tagName==='TEXTAREA');comment.value='Hold this picture';comment.listeners.input();
 button(cards()[0],'Remove').click();assert.equal(cards().length,3);assert.equal(element('save').disabled,true);
 assert.equal(button(cards()[0],'Remove').disabled,true);
 element('fit').click();assert.equal(element('save').disabled,false);element('save').click();
 const draft=JSON.parse(await downloaded.text());assert.equal(draft.schema,scoped?'reel.cel-sorter-edit.v4':'reel.cel-sorter-edit.v3');assert.equal(draft.language,scoped?'es':undefined);assert.equal(element('language-scope').hidden,!scoped);if(scoped)assert.match(element('language-scope').textContent,/Editing es/);assert.equal(draft.rows[0].samples,96);assert.equal(draft.rows[0].comment,'Hold this picture');assert.equal(draft.rows[0].picture_binding,'candidate');
 if(controls){assert.equal(animation,undefined);assert.equal(element('audio').src,undefined);assert.equal(element('preview').src,undefined);assert.equal(element('audio').hidden,true);assert.equal(element('media-notice').hidden,false);element('next').click();assert.match(element('clock').textContent,/Cel 2 of/);assert(descendants(cards()[0]).filter(n=>n.tagName==='IMG').every(n=>n.src===undefined));}
 else{element('audio').currentTime=2;animation();assert(element('preview').alt.includes('c'));}
 element('undo').click();element('undo').click();assert.equal(cards().length,4);
 assert(!String(element('clock').textContent).includes('NaN'));
});

test('split audition switches only the proposal picture, stops without cue advance and cancels pending playback',async()=>{
 class Element{
  constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.dataset={};this.value='';this.classList={toggle(){},add(){},remove(){}};this.paused=true;this.readyState=1;this.currentTime=0;this.duration=2;}
  append(...items){this.children.push(...items);}replaceChildren(...items){this.children=items;}focus(){}setAttribute(k,v){this[k]=v;}
  addEventListener(name,fn,opts={}){(this.listeners[name]??=[]).push({fn,once:opts.once});}emit(name){const all=this.listeners[name]||[];this.listeners[name]=all.filter(e=>!e.once);for(const e of all)e.fn({target:this});}click(){this.emit('click');this.onclick?.();}
  pause(){this.paused=true;}play(){this.paused=false;this.emit('play');return Promise.resolve();}load(){this.readyState=0;this.currentTime=0;}
 }
 const elements=new Map(),el=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};const document={getElementById:el,createElement:t=>new Element(t),querySelectorAll:()=>el('filmstrip').children};const rows=[{semantic_id:'a',cue_id:'first',picture_binding:'one',samples:96,sample_rate:48},{semantic_id:'b',cue_id:'second',picture_binding:'two',samples:88,sample_rate:44}],recordings=rows.map((r,i)=>({cue_id:r.cue_id,samples:r.samples,sample_rate:r.sample_rate,start_seconds:i*2,sha256:String(i+1).repeat(64),src:'media/'+String(i+1).repeat(64)+'.wav',bytes:100}));const session={schema:'reel.cel-sorter-session.v4',language:'es',expected_project:'a'.repeat(64),expected_inputs:{scene:'locked'},scene_id:'test',rows,images:{one:'one.png',two:'two.png'},picture_choices:['one','two'],cues:recordings,recordings,media_mode:'native-recordings',movie:null,total_seconds:4,fps_numerator:24,fps_denominator:1,insertion:{schema:'reel.cel-sorter-insertion-context.v1',language:'es',anchors:rows.map(r=>({semantic_id:r.semantic_id,cue_id:r.cue_id,canonical_cue_id:'canonical-'+r.cue_id}))}};let tick,download;
 vm.runInNewContext(fs.readFileSync(require.resolve('../src/cel_sorter.js'),'utf8'),{document,fetch:async()=>({ok:true,json:async()=>session}),structuredClone,Blob,TextEncoder,crypto:{randomUUID:()=>'synthetic-uuid'},URL:{createObjectURL:b=>{download=b;return 'blob:synthetic';},revokeObjectURL(){}},localStorage:{getItem(){},setItem(){}},requestAnimationFrame:fn=>tick=fn,setTimeout:fn=>fn(),confirm:()=>true});await new Promise(setImmediate);
 const a=el('audio');el('new-cel').click();el('insert-score').value='silence';el('insert-picture').value='two';el('insert-picture').onchange();assert.equal(el('insert-preview').src,'two.png');assert.match(el('insert-summary').textContent,/Keep 48 samples.*new cel 48/);el('insert-play').click();await new Promise(setImmediate);assert.equal(a.paused,false);assert.equal(el('insert-preview').src,'one.png');a.currentTime=1.1;a.emit('timeupdate');assert.equal(el('insert-preview').src,'two.png');assert.equal(el('filmstrip').children.length,2);a.currentTime=2;a.emit('timeupdate');assert(a.paused);a.emit('ended');assert.equal(a.src,recordings[0].src);assert(el('insert-stop').disabled);
 // The next cue can be auditioned, and Stop invalidates its pending load callback.
 el('insert-after').value='b';el('insert-after').onchange();el('insert-play').click();assert.equal(a.readyState,0);el('insert-stop').click();a.readyState=1;a.emit('loadedmetadata');await new Promise(setImmediate);assert(a.paused);
 el('insert-play').click();await new Promise(setImmediate);assert(!a.paused);el('insert-split').value='22';el('insert-split').oninput();assert(a.paused);assert.match(el('insert-summary').textContent,/0.500 s/);
 // Closing or editing review feedback also stops the audition, without changing holds.
 el('insert-play').click();el('insertion-panel').open=false;el('insertion-panel').emit('toggle');assert(a.paused);el('insert-play').click();const comment=el('filmstrip').children[0].children[1].children.at(-1).children[0];comment.value='Keep this review';comment.emit('input');assert(a.paused);assert(el('insert-play').disabled);el('insert-play').click();assert.match(el('status').textContent,/Save and apply/);el('save').click();const saved=JSON.parse(await download.text());assert.equal(saved.rows.length,2);assert.equal(saved.rows[0].samples,96);assert.equal(saved.rows[0].comment,'Keep this review');tick();assert.equal(session.rows[0].samples,96);
});
