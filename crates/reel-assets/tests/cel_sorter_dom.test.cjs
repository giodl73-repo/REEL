const test=require('node:test'),assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
// Execute the exported editor and its handlers against a minimal synthetic DOM.
// This tests UI wiring; it is not a browser layout, media or visual acceptance test.
for(const controls of [false,true])for(const scoped of [false,true])test('exported multi-cue UI preserves scope and comments '+(scoped?'v4':'v3')+(controls?' controls-only':' media'),async()=>{
 class Element {
  constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.dataset={};this.value='';this.classList={toggle(){},add(){},remove(){}};}
  append(...items){this.children.push(...items);}replaceChildren(...items){this.children=items;}
  addEventListener(name,fn){this.listeners[name]=fn;}setAttribute(name,value){this[name]=value;}
  click(){this.listeners.click?.({target:this});this.onclick?.();}
 }
 const elements=new Map(),element=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};
 const document={getElementById:element,createElement:tag=>new Element(tag),querySelectorAll:()=>element('filmstrip').children};
 const rows=[{semantic_id:'a',picture_binding:'one',cue_id:'first',samples:47,sample_rate:48},{semantic_id:'b',picture_binding:'two',cue_id:'first',samples:49,sample_rate:48},{semantic_id:'c',picture_binding:'one',cue_id:'second',samples:43,sample_rate:44},{semantic_id:'d',picture_binding:'two',cue_id:'second',samples:45,sample_rate:44}];
 const session={schema:scoped?'reel.cel-sorter-session.v4':'reel.cel-sorter-session.v3',...(scoped?{language:'es'}:{}),expected_project:'a'.repeat(64),expected_inputs:{scene:'synthetic'},scene_id:'synthetic',rows,images:{one:'one.png',two:'two.png'},delivery_clock:{state:'mixed-native-rates'},total_seconds:4,fps_numerator:24,fps_denominator:1,movie:'synthetic.mp4'};
 if(controls){session.media_mode='controls-only';session.images={};session.movie=null;}
 let downloaded,animation;const saved=new Map();
 vm.runInNewContext(fs.readFileSync(require.resolve('../src/cel_sorter.js'),'utf8'),{document,fetch:async()=>({ok:true,json:async()=>session}),structuredClone,Blob,TextEncoder,URL:{createObjectURL:blob=>{downloaded=blob;return 'blob:synthetic';},revokeObjectURL(){}},localStorage:{getItem:key=>saved.get(key),setItem:(key,value)=>saved.set(key,value)},requestAnimationFrame:fn=>{animation=fn;},setTimeout:fn=>fn(),confirm:()=>true});
 await new Promise(setImmediate);
 const cards=()=>element('filmstrip').children;
 const descendants=node=>[node,...node.children.flatMap(descendants)];
 const button=(card,label)=>descendants(card).find(node=>node.tagName==='BUTTON'&&node.textContent===label);
 assert.equal(cards().length,4);assert.equal(element('save').disabled,false);assert.equal(element('delivery-clock').hidden,false);assert.match(element('delivery-clock').textContent,/different sample rates/);
 assert(descendants(cards()[2]).some(node=>node.textContent?.startsWith('2.00 s')));
 assert.equal(button(cards()[1],'\u2193').disabled,true);assert.equal(button(cards()[2],'\u2191').disabled,true);
 // Dragging across cues rolls back the actual UI state.
 cards()[0].listeners.dragstart({target:cards()[0],dataTransfer:{setData(){}},preventDefault(){}});
 cards()[2].listeners.drop({preventDefault(){}});assert.equal(cards().length,4);
 assert(descendants(cards()[0]).some(node=>node.textContent?.includes('first \u00b7 a')));
 // Comments follow the remaining cel through removal, fit and download.
 const comment=descendants(cards()[1]).find(node=>node.tagName==='TEXTAREA');comment.value='Hold this picture';comment.listeners.input();
 button(cards()[0],'Remove').click();assert.equal(cards().length,3);assert.equal(element('save').disabled,true);
 assert.equal(button(cards()[0],'Remove').disabled,true);
 element('fit').click();assert.equal(element('save').disabled,false);element('save').click();
 const draft=JSON.parse(await downloaded.text());assert.equal(draft.schema,scoped?'reel.cel-sorter-edit.v4':'reel.cel-sorter-edit.v3');assert.equal(draft.language,scoped?'es':undefined);assert.equal(element('language-scope').hidden,!scoped);if(scoped)assert.match(element('language-scope').textContent,/Editing es/);assert.equal(draft.rows[0].samples,96);assert.equal(draft.rows[0].comment,'Hold this picture');
 if(controls){assert.equal(animation,undefined);assert.equal(element('audio').src,undefined);assert.equal(element('preview').src,undefined);assert.equal(element('audio').hidden,true);assert.equal(element('media-notice').hidden,false);element('next').click();assert.match(element('clock').textContent,/Cel 2 of/);assert(descendants(cards()[0]).filter(n=>n.tagName==='IMG').every(n=>n.src===undefined));}
 else{element('audio').currentTime=2;animation();assert(element('preview').alt.includes('c'));}
 element('undo').click();element('undo').click();assert.equal(cards().length,4);
 assert(!String(element('clock').textContent).includes('NaN'));
});
