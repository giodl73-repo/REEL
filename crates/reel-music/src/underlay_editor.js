'use strict';
const UnderlayEditor={
 bytes(text){return new TextEncoder().encode(text);},
 nonempty(text){return typeof text==='string'&&!/^[\u0009-\u000d\u0020\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]*$/u.test(text);},
 key(s){return 'reel-music-underlay:v1:'+JSON.stringify([s.edit_request.expected_model_sha256,s.edit_request.lyric_layer_id,s.edit_request.expected_lyric_sha256]);},
 // Textarea line-ending normalization changes display indices, never source bytes.
 display(source){let text='',bytes=0;const boundaries=[0];for(let i=0;i<source.length;){const c=String.fromCodePoint(source.codePointAt(i)),n=c==='\r'&&source[i+1]==='\n'?2:c.length,shown=c==='\r'?'\n':c;bytes+=this.bytes(source.slice(i,i+n)).length;for(let j=1;j<shown.length;j++)boundaries.push(null);boundaries.push(bytes);text+=shown;i+=n;}return {text,boundaries};},
 selection(source,start,end){const map=this.display(source).boundaries;if(!Number.isInteger(start)||!Number.isInteger(end)||start<0||start>=end||end>=map.length||map[start]===null||map[end]===null)throw Error('Select complete source characters');const a=map[start],b=map[end];this.spanText(source,a,b);return {text_start_byte:a,text_end_byte:b};},
 spanText(source,start,end){const raw=this.bytes(source);if(!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start<0||start>=end||end>raw.length)throw Error('Choose a non-empty source span');const text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(raw.slice(start,end));if(!this.nonempty(text))throw Error('Select lyric text, not only whitespace');return text;},
 sameUnderlay(a,b){const values=rows=>rows.map(r=>[r.id,r.note_ids,r.text_start_byte,r.text_end_byte,r.syllabic]);return JSON.stringify(values(a))===JSON.stringify(values(b));},
 matches(s,e){const fields=['schema','expected_model_sha256','expected_lyric_sha256','lyric_layer_id','candidate_model_id','underlay','rationale'];return s.schema==='reel.music-underlay-session.v1'&&e?.schema==='reel.music-underlay-edit.v1'&&Object.keys(e).sort().join()===fields.sort().join()&&['expected_model_sha256','expected_lyric_sha256','lyric_layer_id'].every(k=>e[k]===s.edit_request[k]);},
 valid(s,e){try{
  if(!this.matches(s,e)||!this.nonempty(e.candidate_model_id)||this.bytes(e.candidate_model_id).length>256||e.candidate_model_id===s.model_id||typeof e.rationale!=='string'||this.bytes(e.rationale).length>4096||!Array.isArray(e.underlay)||!e.underlay.length||e.underlay.length>1000)return false;
  if(!this.sameUnderlay(e.underlay,s.edit_request.underlay)&&!this.nonempty(e.rationale))return false;
  const ids=new Set(),mapped=new Set(),notes=new Map(s.notes.map(n=>[n.id,n.start_tick]));let textEnd=0,priorTick=0,refs=0;
  for(const row of e.underlay){if(Object.keys(row).sort().join()!==['id','note_ids','text_start_byte','text_end_byte','syllabic'].sort().join()||!this.nonempty(row.id)||this.bytes(row.id).length>256||ids.has(row.id)||!['single','begin','middle','end'].includes(row.syllabic)||!Array.isArray(row.note_ids)||!row.note_ids.length)return false;ids.add(row.id);
   this.spanText(s.lyric_text,row.text_start_byte,row.text_end_byte);if(row.text_start_byte<textEnd)return false;textEnd=row.text_end_byte;
   for(const id of row.note_ids){const tick=notes.get(id);if(tick===undefined||mapped.has(id)||(mapped.size&&tick<priorTick))return false;mapped.add(id);priorTick=tick;if(++refs>1000)return false;}
  }
  return mapped.size===notes.size&&this.bytes(JSON.stringify(e)).length<=2*1024*1024;
 }catch{return false;}}
};
if(typeof module!=='undefined')module.exports=UnderlayEditor;
if(typeof document!=='undefined')(async()=>{
 const $=id=>document.getElementById(id),session=await fetch('session.json').then(r=>{if(!r.ok)throw Error('Could not load underlay session');return r.json();});
 let edit=structuredClone(session.edit_request),selected=0,page=0;const history=[],key=UnderlayEditor.key(session),display=UnderlayEditor.display(session.lyric_text);
 if(!UnderlayEditor.matches(session,edit)||!session.notes.length)throw Error('Unsupported underlay session');
 try{const saved=JSON.parse(localStorage.getItem(key));if(UnderlayEditor.valid(session,saved))edit=saved;}catch{}
 $('lyrics').value=display.text;$('model').textContent=session.model_id+' · '+session.edit_request.lyric_layer_id+' · '+session.language+' · '+session.lyric_kind+' · exact lyric bytes verified; no audio loaded';
 for(const note of session.notes){const option=document.createElement('option');option.value=note.id;option.label=note.id+' · pitch '+note.midi_note+' · tick '+note.start_tick;$('note-choices').append(option);}
 const message=text=>{$('status').textContent=text;},remember=()=>{history.push(structuredClone(edit));if(history.length>30)history.shift();};
 function fields(){$('candidate').value=edit.candidate_model_id;$('rationale').value=edit.rationale;}
 function update(){const valid=UnderlayEditor.valid(session,edit);$('save').disabled=!valid;$('use').disabled=!edit.underlay.length;$('add').disabled=edit.underlay.length>=1000;$('undo').disabled=!history.length;message(valid?'Ready to save an unreviewed underlay draft.':'Map every melody note once, keep spans in source order, and explain the proposal.');if(valid)try{localStorage.setItem(key,JSON.stringify(edit));}catch{message('Browser storage is unavailable. Save a draft to keep your work.');}}
 function focusSpan(){const row=edit.underlay[selected];if(!row)return;const a=display.boundaries.indexOf(row.text_start_byte),b=display.boundaries.indexOf(row.text_end_byte);if(a>=0&&b>a){$('lyrics').focus();$('lyrics').setSelectionRange(a,b);}else message('Select this span again from the source text to change its boundaries.');}
 function render(){const pages=Math.max(1,Math.ceil(edit.underlay.length/100));page=Math.min(page,pages-1);selected=Math.max(0,Math.min(selected,edit.underlay.length-1));$('spans').replaceChildren();
  for(let index=page*100;index<Math.min(edit.underlay.length,(page+1)*100);index++){const row=edit.underlay[index],card=document.createElement('section');card.className='span'+(index===selected?' active':'');const label=document.createElement('strong');label.textContent=row.id;card.append(label);
   const text=document.createElement('p');text.className='text';try{text.textContent=UnderlayEditor.spanText(session.lyric_text,row.text_start_byte,row.text_end_byte);}catch{text.textContent='Choose source text for this span';}card.append(text);
   const toolbar=document.createElement('div');toolbar.className='toolbar';
   const button=(title,handler,disabled=false)=>{const node=document.createElement('button');node.textContent=title;node.disabled=disabled;node.addEventListener('click',handler);toolbar.append(node);};
   button('Select span',()=>{selected=index;render();focusSpan();});
   button('↑',()=>{remember();[edit.underlay[index-1],edit.underlay[index]]=[row,edit.underlay[index-1]];selected=index-1;page=Math.floor(selected/100);render();update();},index===0);
   button('↓',()=>{remember();[edit.underlay[index+1],edit.underlay[index]]=[row,edit.underlay[index+1]];selected=index+1;page=Math.floor(selected/100);render();update();},index===edit.underlay.length-1);
   button('Remove span',()=>{remember();edit.underlay.splice(index,1);selected=Math.min(index,edit.underlay.length-1);render();update();});card.append(toolbar);
   const select=(title,values,current)=>{const wrap=document.createElement('label'),node=document.createElement('select');wrap.textContent=title+' ';node.setAttribute('aria-label',row.id+' '+title);for(const [value,label] of values){const option=document.createElement('option');option.value=value;option.textContent=label;node.append(option);}node.value=current;wrap.append(node);card.append(wrap);return node;};
   const noteInput=(title,current)=>{const wrap=document.createElement('label'),node=document.createElement('input');wrap.textContent=title+' ';node.value=current;node.setAttribute('list','note-choices');node.setAttribute('aria-label',row.id+' '+title);wrap.append(node);card.append(wrap);return node;};
   const first=noteInput('First note',row.note_ids[0]||''),last=noteInput('Last note',row.note_ids.at(-1)||'');
   const range=()=>{const a=session.notes.findIndex(n=>n.id===first.value),b=session.notes.findIndex(n=>n.id===last.value);if(a<0||b<a){first.value=row.note_ids[0]||'';last.value=row.note_ids.at(-1)||'';message('Choose known notes, with the first at or before the last');return;}remember();row.note_ids=session.notes.slice(a,b+1).map(n=>n.id);selected=index;Array.from($('spans').children).forEach((node,i)=>{node.className='span'+(page*100+i===selected?' active':'');});update();};first.addEventListener('change',range);last.addEventListener('change',range);
   select('Syllable treatment',['single','begin','middle','end'].map(x=>[x,x]),row.syllabic).addEventListener('change',event=>{remember();row.syllabic=event.target.value;update();});$('spans').append(card);
  }
  $('page').textContent=`Page ${page+1} of ${pages}`;$('previous').disabled=page===0;$('next').disabled=page===pages-1;
 }
 $('use').addEventListener('click',()=>{try{const span=UnderlayEditor.selection(session.lyric_text,$('lyrics').selectionStart,$('lyrics').selectionEnd);if(!edit.underlay[selected])return;remember();Object.assign(edit.underlay[selected],span);render();update();}catch(error){message(error.message);}});
 $('add').addEventListener('click',()=>{if(edit.underlay.length>=1000)return;remember();const ids=new Set([...edit.underlay,...session.edit_request.underlay].map(r=>r.id));let n=1;while(ids.has('span-'+n))n++;const used=new Set(edit.underlay.flatMap(r=>r.note_ids)),note=session.notes.find(n=>!used.has(n.id))||session.notes.at(-1);let span={text_start_byte:0,text_end_byte:0};try{span=UnderlayEditor.selection(session.lyric_text,$('lyrics').selectionStart,$('lyrics').selectionEnd);}catch{}edit.underlay.push({id:'span-'+n,note_ids:[note.id],...span,syllabic:'single'});selected=edit.underlay.length-1;page=Math.floor(selected/100);render();update();});
 $('candidate').addEventListener('change',()=>{remember();edit.candidate_model_id=$('candidate').value;update();});$('rationale').addEventListener('change',()=>{remember();edit.rationale=$('rationale').value;update();});
 $('undo').addEventListener('click',()=>{if(!history.length)return;edit=history.pop();fields();render();update();});$('reset').addEventListener('click',()=>{remember();edit=structuredClone(session.edit_request);page=0;selected=0;fields();render();update();});
 $('previous').addEventListener('click',()=>{page--;selected=page*100;render();});$('next').addEventListener('click',()=>{page++;selected=page*100;render();});
 $('save').addEventListener('click',()=>{if(!UnderlayEditor.valid(session,edit))return;const url=URL.createObjectURL(new Blob([JSON.stringify(edit)],{type:'application/json'})),a=document.createElement('a');a.href=url;a.download='music-underlay-edit.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
 $('open').addEventListener('click',()=>$('file').click());$('file').addEventListener('change',async()=>{try{const file=$('file').files[0];if(!file)return;if(file.size>2*1024*1024)throw Error('Draft exceeds the 2 MiB limit');const candidate=JSON.parse(await file.text());if(!UnderlayEditor.valid(session,candidate))throw Error('Draft does not match this model/lyric layer or has invalid spans');remember();edit=candidate;page=0;selected=0;fields();render();update();}catch(error){message(error.message);}finally{$('file').value='';}});
 fields();render();update();
})().catch(error=>{document.getElementById('status').textContent=error.message;});
