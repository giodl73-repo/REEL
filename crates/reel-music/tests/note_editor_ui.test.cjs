const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const Editor=require('../src/note_editor.js');
function fixture(count=4){
 const rows=Array.from({length:count},(_,i)=>({part_id:'melody',note_id:'n'+i,midi_note:60+i%12,start_tick:i*10,duration_ticks:10,rationale:''}));
 return {schema:'reel.music-note-session.v1',model_id:'synthetic',duration_ticks:count*10,pulses_per_quarter:10,edit_request:{schema:'reel.music-note-edit.v1',expected_model_sha256:'a'.repeat(64),candidate_model_id:'synthetic-proposal',rows},parts:[{id:'melody',name:'<img src=x onerror=alert(1)>',role:'melody',notes:rows.map(r=>({id:r.note_id,voice:1}))}],lead_sheet:{melody_part_id:'melody',underlay:[{id:'syllable',note_ids:['n0']}]}};
}
test('locked note drafts enforce complete IDs, byte budgets, rationale and tick/voice constraints',()=>{
 const session=fixture(),base=session.edit_request;assert(Editor.valid(session,base));
 for(const mutate of [e=>e.expected_model_sha256='b'.repeat(64),e=>e.schema='reel.music-note-edit.v2',e=>e.candidate_model_id='synthetic',e=>e.candidate_model_id='á'.repeat(129),e=>e.rows.pop(),e=>e.rows[1]=structuredClone(e.rows[0]),e=>e.human_approval=true,e=>e.rows[0].midi_note=128,e=>e.rows[0].midi_note=61,e=>e.rows[0].duration_ticks=0,e=>e.rows[0].duration_ticks=11,e=>e.rows[0].start_tick=NaN,e=>e.rows[0].rationale='á'.repeat(2049),e=>e.rows[0].velocity=20]){
  const edit=structuredClone(base);mutate(edit);assert(!Editor.valid(session,edit));
 }
 const edit=structuredClone(base);edit.rows[0].midi_note=61;edit.rows[0].duration_ticks=9;edit.rows[0].rationale='Compare release';assert(Editor.valid(session,edit));
 assert.notEqual(Editor.key(session),Editor.key({...session,edit_request:{...base,expected_model_sha256:'b'.repeat(64)}}));
 assert.equal(Editor.pitch(60),'C4');assert.equal(Editor.pitch(61),'C♯4');
});
async function harness(session=fixture()){
 class Element{
  constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.value='';this.textContent='';this.disabled=false;}
  append(...nodes){this.children.push(...nodes);}replaceChildren(...nodes){this.children=nodes;}setAttribute(k,v){this[k]=v;}
  addEventListener(k,v){this.listeners[k]=v;}click(){if(!this.disabled)return this.listeners.click?.();}
 }
 const elements=new Map(),element=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};let downloaded;
 const saved=new Map();vm.runInNewContext(fs.readFileSync(require.resolve('../src/note_editor.js'),'utf8'),{
  document:{getElementById:element,createElement:tag=>new Element(tag),createElementNS:(_,tag)=>new Element(tag)},
  fetch:async()=>({ok:true,json:async()=>session}),structuredClone,Blob,TextEncoder,
  localStorage:{getItem:k=>saved.get(k),setItem:(k,v)=>saved.set(k,v)},
  URL:{createObjectURL:blob=>{downloaded=blob;return 'blob:synthetic';},revokeObjectURL(){}},setTimeout:fn=>fn()
 });await new Promise(setImmediate);
 const descendants=node=>[node,...node.children.flatMap(descendants)];
 const input=label=>descendants(element('rows')).find(n=>n['aria-label']===label);
 const change=(label,value)=>{const node=input(label);node.value=value;return node.listeners.change();};
 return {element,input,change,saved,download:async()=>JSON.parse(await downloaded.text())};
}
test('actual exported handlers preserve edits through save/import/undo and render hostile labels as text',async()=>{
 const h=await harness();assert.equal(h.element('rows').children.length,4);assert.equal(h.element('save').disabled,false);
 assert.equal(h.element('part').children[0].textContent,'<img src=x onerror=alert(1)> · melody');assert.equal(h.element('roll').children.length,4);
 h.change('n0 midi note','61');assert.equal(h.element('save').disabled,true);
 h.change('n0 rationale','Try a semitone higher');assert.equal(h.element('save').disabled,false);
 h.element('save').click();const edit=await h.download();assert.equal(edit.rows[0].midi_note,61);assert.equal(edit.rows[0].rationale,'Try a semitone higher');
 const lastSaved=h.saved.get(Editor.key(fixture()));h.change('n0 duration ticks','11');assert.equal(h.element('save').disabled,true);assert.equal(h.saved.get(Editor.key(fixture())),lastSaved);
 h.element('undo').click();assert.equal(h.input('n0 duration ticks').value,10);
 h.element('reset').click();assert.equal(h.input('n0 midi note').value,60);
 h.element('file').files=[{size:JSON.stringify(edit).length,text:async()=>JSON.stringify(edit)}];await h.element('file').listeners.change();assert.equal(h.input('n0 midi note').value,61);
 const stale={...edit,expected_model_sha256:'b'.repeat(64)};h.element('file').files=[{size:100,text:async()=>JSON.stringify(stale)}];await h.element('file').listeners.change();assert.match(h.element('status').textContent,/does not match/);assert.equal(h.input('n0 midi note').value,61);
 h.element('file').files=[{size:2*1024*1024+1}];await h.element('file').listeners.change();assert.match(h.element('status').textContent,/2 MiB/);
});
test('large native parts paginate note controls to one hundred rows',async()=>{
 const h=await harness(fixture(205));assert.equal(h.element('rows').children.length,100);assert.equal(h.element('previous').disabled,true);
 h.element('next').click();assert.equal(h.element('rows').children.length,100);h.element('next').click();assert.equal(h.element('rows').children.length,5);assert.equal(h.element('next').disabled,true);
 h.element('previous').click();assert.equal(h.element('rows').children.length,100);
});
