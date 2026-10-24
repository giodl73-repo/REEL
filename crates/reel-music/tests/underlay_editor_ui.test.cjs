const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const Editor=require('../src/underlay_editor.js');
function fixture(count=4){
 const notes=Array.from({length:count},(_,i)=>({id:'note-'+(i+1),start_tick:i*960,duration_ticks:960,midi_note:60}));
 const underlay=notes.map((n,i)=>({id:'span-'+(i+1),note_ids:[n.id],text_start_byte:i*3,text_end_byte:i*3+2,syllabic:'single'}));
 return {schema:'reel.music-underlay-session.v1',model_id:'synthetic',lyric_text:Array(count).fill('la').join(' ')+'\n',language:'und',lyric_kind:'canonical',notes,edit_request:{schema:'reel.music-underlay-edit.v1',expected_model_sha256:'a'.repeat(64),expected_lyric_sha256:'b'.repeat(64),lyric_layer_id:'canonical',candidate_model_id:'proposal',underlay,rationale:''}};
}
test('display selections preserve raw UTF-8 offsets through accents, emoji, BOM and CRLF',()=>{
 const source='Á😀\r\nñ';const display=Editor.display(source);assert.equal(display.text,'Á😀\nñ');assert.deepEqual(display.boundaries,[0,2,null,6,8,10]);
 assert.deepEqual(Editor.selection(source,1,3),{text_start_byte:2,text_end_byte:6});
 assert.deepEqual(Editor.selection(source,3,5),{text_start_byte:6,text_end_byte:10});
 assert.throws(()=>Editor.selection(source,2,3));assert.throws(()=>Editor.spanText(source,3,6));assert.throws(()=>Editor.spanText(source,6,8));
 assert.equal(Editor.spanText('\ufeffx',0,3),'\ufeff');assert.equal(Editor.nonempty('\u0085'),false);
 assert.equal(Editor.display('a\rb').text,'a\nb');
});
test('source locks, full note coverage, UTF-8 spans and the first melisma order are strict',()=>{
 const session=fixture();assert(Editor.valid(session,session.edit_request));
 for(const mutate of [e=>e.expected_model_sha256='c'.repeat(64),e=>e.expected_lyric_sha256='c'.repeat(64),e=>e.lyric_layer_id='foreign',e=>e.schema='reel.music-note-edit.v1',e=>e.underlay.pop(),e=>e.underlay[1].note_ids=['note-1'],e=>e.underlay[0].note_ids=['foreign'],e=>e.underlay[1].text_start_byte=0,e=>e.underlay[0].syllabic='invented',e=>e.human_approval=true,e=>e.underlay[0].id='',e=>e.candidate_model_id='synthetic']){const edit=structuredClone(session.edit_request);edit.rationale='Synthetic test';mutate(edit);assert(!Editor.valid(session,edit));}
 const edit=structuredClone(session.edit_request);edit.underlay[0].note_ids.push('note-2');edit.underlay[0].text_end_byte=5;edit.underlay.splice(1,1);assert(!Editor.valid(session,edit));edit.rationale='Source phrase across two notes';assert(Editor.valid(session,edit));edit.underlay[0].note_ids.reverse();assert(!Editor.valid(session,edit));
 const reordered=structuredClone(session.edit_request);reordered.underlay=reordered.underlay.map(r=>({syllabic:r.syllabic,text_end_byte:r.text_end_byte,text_start_byte:r.text_start_byte,note_ids:r.note_ids,id:r.id}));assert(Editor.valid(session,reordered));
 assert.notEqual(Editor.key(session),Editor.key({...session,edit_request:{...session.edit_request,lyric_layer_id:'other'}}));
});
async function harness(session=fixture()){
 class Element{constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.listeners={};this.value='';this.textContent='';this.disabled=false;this.selectionStart=0;this.selectionEnd=0;}
 append(...nodes){this.children.push(...nodes);}replaceChildren(...nodes){this.children=nodes;}setAttribute(k,v){this[k]=v;}addEventListener(k,v){this.listeners[k]=v;}click(){if(!this.disabled)return this.listeners.click?.();}focus(){}setSelectionRange(a,b){this.selectionStart=a;this.selectionEnd=b;}}
 const elements=new Map(),element=id=>{if(!elements.has(id))elements.set(id,new Element());return elements.get(id);};let downloaded;const saved=new Map();
 vm.runInNewContext(fs.readFileSync(require.resolve('../src/underlay_editor.js'),'utf8'),{document:{getElementById:element,createElement:tag=>new Element(tag)},fetch:async()=>({ok:true,json:async()=>session}),structuredClone,TextEncoder,TextDecoder,Blob,localStorage:{getItem:k=>saved.get(k),setItem:(k,v)=>saved.set(k,v)},URL:{createObjectURL:blob=>{downloaded=blob;return 'blob:synthetic';},revokeObjectURL(){}},setTimeout:fn=>fn()});await new Promise(setImmediate);
 const descendants=node=>[node,...node.children.flatMap(descendants)],button=(index,title)=>descendants(element('spans').children[index]).find(n=>n.tagName==='BUTTON'&&n.textContent===title),input=label=>descendants(element('spans')).find(n=>n['aria-label']===label);
 return {element,button,input,saved,download:async()=>JSON.parse(await downloaded.text())};
}
test('exported handlers merge exact spans, save/import/undo and refuse stale drafts',async()=>{
 const h=await harness();assert.equal(h.element('spans').children.length,4);assert.equal(h.element('note-choices').children.length,4);assert.equal(h.element('save').disabled,false);
 h.button(0,'Select span').click();assert.equal(h.element('lyrics').selectionEnd,2);h.element('lyrics').selectionEnd=5;h.element('use').click();assert.equal(h.element('save').disabled,true);
 const lastInput=h.input('span-1 Last note'),syllabicInput=h.input('span-1 Syllable treatment');
 lastInput.value='note-2';lastInput.listeners.change();assert.equal(h.input('span-1 Last note'),lastInput);assert.equal(h.input('span-1 Syllable treatment'),syllabicInput);
 syllabicInput.value='begin';syllabicInput.listeners.change({target:syllabicInput});assert.equal(h.input('span-1 Syllable treatment'),syllabicInput);h.button(1,'Remove span').click();
 h.element('rationale').value='Whole phrase over the first two notes';h.element('rationale').listeners.change();assert.equal(h.element('save').disabled,false);h.element('save').click();const edit=await h.download();assert.equal(edit.underlay.length,3);assert.deepEqual(edit.underlay[0].note_ids,['note-1','note-2']);assert.equal(edit.underlay[0].text_end_byte,5);assert.equal(edit.lyric_text,undefined);
 h.element('reset').click();assert.equal(h.element('spans').children.length,4);h.element('undo').click();assert.equal(h.element('spans').children.length,3);
 h.element('file').files=[{size:100,text:async()=>JSON.stringify({...edit,expected_lyric_sha256:'c'.repeat(64)})}];await h.element('file').listeners.change();assert.match(h.element('status').textContent,/does not match/);assert.equal(h.element('spans').children.length,3);
 h.element('reset').click();h.element('file').files=[{size:JSON.stringify(edit).length,text:async()=>JSON.stringify(edit)}];await h.element('file').listeners.change();assert.equal(h.element('spans').children.length,3);
 h.input('span-1 First note').value='unknown';h.input('span-1 First note').listeners.change();assert.equal(h.input('span-1 First note').value,'note-1');
});
test('incomplete mappings remain editable and large span sets use one bounded shared note list',async()=>{
 const session=fixture();session.edit_request.underlay=[];const h=await harness(session);assert.equal(h.element('save').disabled,true);h.element('add').click();assert.equal(h.element('spans').children.length,1);h.element('lyrics').selectionStart=0;h.element('lyrics').selectionEnd=11;h.element('use').click();
 h.input('span-1 Last note').value='note-4';h.input('span-1 Last note').listeners.change();h.element('rationale').value='Complete exact source phrase';h.element('rationale').listeners.change();assert.equal(h.element('save').disabled,false);
 const large=await harness(fixture(205));assert.equal(large.element('spans').children.length,100);assert.equal(large.element('note-choices').children.length,205);large.element('next').click();large.element('next').click();assert.equal(large.element('spans').children.length,5);assert.equal(large.element('next').disabled,true);
});
