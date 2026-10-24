'use strict';
const NoteEditor={
 key(session){return 'reel-music-note-editor:v1:'+session.edit_request.expected_model_sha256;},
 bytes(text){return new TextEncoder().encode(text).length;},
 pitch(n){return ['C','C♯','D','D♯','E','F','F♯','G','G♯','A','A♯','B'][n%12]+(Math.floor(n/12)-1);},
 valid(session,edit){
  if(session.schema!=='reel.music-note-session.v1'||edit?.schema!=='reel.music-note-edit.v1'||edit.expected_model_sha256!==session.edit_request.expected_model_sha256)return false;
  if(Object.keys(edit).sort().join()!==['schema','expected_model_sha256','candidate_model_id','rows'].sort().join())return false;
  if(typeof edit.candidate_model_id!=='string'||!edit.candidate_model_id.trim()||this.bytes(edit.candidate_model_id)>256||edit.candidate_model_id===session.model_id)return false;
  const known=session.edit_request.rows;
  if(!Array.isArray(edit.rows)||edit.rows.length!==known.length)return false;
  const seen=new Set(),ends=new Map();
  for(const original of known){
   const row=edit.rows.find(r=>r.part_id===original.part_id&&r.note_id===original.note_id);
   if(!row||Object.keys(row).sort().join()!==['part_id','note_id','midi_note','start_tick','duration_ticks','rationale'].sort().join())return false;
   const id=JSON.stringify([row.part_id,row.note_id]);if(seen.has(id))return false;seen.add(id);
   if(!Number.isSafeInteger(row.midi_note)||row.midi_note<0||row.midi_note>127||!Number.isSafeInteger(row.start_tick)||row.start_tick<0||!Number.isSafeInteger(row.duration_ticks)||row.duration_ticks<1||!Number.isSafeInteger(row.start_tick+row.duration_ticks)||row.start_tick+row.duration_ticks>session.duration_ticks)return false;
   if(typeof row.rationale!=='string'||this.bytes(row.rationale)>4096)return false;
   const changed=['midi_note','start_tick','duration_ticks'].some(k=>row[k]!==original[k]);if(changed&&!row.rationale.trim())return false;
   const part=session.parts.find(p=>p.id===row.part_id),voice=part?.notes.find(n=>n.id===row.note_id)?.voice;if(!voice)return false;
   const lane=JSON.stringify([row.part_id,voice]);if((ends.get(lane)||0)>row.start_tick)return false;ends.set(lane,row.start_tick+row.duration_ticks);
  }
  // Native canonical order is checked in the original part/note order.
  for(const part of session.parts){let prior;
   for(const note of part.notes){const row=edit.rows.find(r=>r.part_id===part.id&&r.note_id===note.id);if(!row)return false;
    const key=[row.start_tick,note.voice,row.midi_note,row.note_id];
    if(prior){let comparison=0;for(let i=0;i<4&&!comparison;i++)comparison=typeof key[i]==='number'?key[i]-prior[i]:(key[i]<prior[i]?-1:key[i]>prior[i]?1:0);if(comparison<=0)return false;}prior=key;
   }
  }
  return this.bytes(JSON.stringify(edit))<=2*1024*1024;
 }
};
if(typeof module!=='undefined')module.exports=NoteEditor;
if(typeof document!=='undefined')(async()=>{
 const $=id=>document.getElementById(id),session=await fetch('session.json').then(r=>{if(!r.ok)throw Error('Could not load note session');return r.json();});
 let edit=structuredClone(session.edit_request),page=0;const history=[],key=NoteEditor.key(session);
 if(!NoteEditor.valid(session,edit))throw Error('Unsupported or invalid note session');
 try{const saved=JSON.parse(localStorage.getItem(key));if(NoteEditor.valid(session,saved))edit=saved;}catch{}
 $('model').textContent=session.model_id;$('clock').textContent=`${session.pulses_per_quarter} ticks per quarter · ${session.duration_ticks} total ticks · declared metadata, no audio loaded`;
 for(const part of session.parts){const option=document.createElement('option');option.value=part.id;option.textContent=part.name+' · '+part.role;$('part').append(option);}
 $('part').value=session.parts[0].id;$('candidate').value=edit.candidate_model_id;
 const message=text=>{$('status').textContent=text;};
 function remember(){history.push(structuredClone(edit));if(history.length>30)history.shift();}
 function persist(){try{localStorage.setItem(key,JSON.stringify(edit));}catch{message('Browser storage is unavailable. Save a draft to keep your work.');}}
 function update(){const valid=NoteEditor.valid(session,edit);$('save').disabled=!valid;$('undo').disabled=!history.length;message(valid?'Ready to save an unreviewed draft.':'Check note ranges, order, overlaps, the new model ID and reasons for changed notes.');if(valid)persist();}
 function render(){
  const selected=$('part').value,part=session.parts.find(p=>p.id===selected),rows=part.notes.map(note=>edit.rows.find(r=>r.part_id===selected&&r.note_id===note.id)),pages=Math.max(1,Math.ceil(rows.length/100));page=Math.min(page,pages-1);
  $('rows').replaceChildren();$('roll').replaceChildren();
  for(const row of rows){const rect=document.createElementNS('http://www.w3.org/2000/svg','rect');const finite=Number.isFinite(row.start_tick)&&Number.isFinite(row.duration_ticks)&&Number.isFinite(row.midi_note);if(!finite)continue;
   rect.setAttribute('x',String(Math.max(0,Math.min(960,row.start_tick/session.duration_ticks*960))));rect.setAttribute('y',String(142-row.midi_note/127*136));rect.setAttribute('width',String(Math.max(2,Math.min(960,row.duration_ticks/session.duration_ticks*960))));rect.setAttribute('height','5');rect.setAttribute('fill','#8ed0bd');const title=document.createElementNS('http://www.w3.org/2000/svg','title');title.textContent=row.note_id+' · '+NoteEditor.pitch(row.midi_note);rect.append(title);$('roll').append(rect);
  }
  for(const row of rows.slice(page*100,(page+1)*100)){const tr=document.createElement('tr'),original=session.edit_request.rows.find(r=>r.part_id===row.part_id&&r.note_id===row.note_id),meta=document.createElement('td');meta.className='note-id';meta.textContent=row.note_id+' · voice '+part.notes.find(n=>n.id===row.note_id).voice;tr.append(meta);if(['midi_note','start_tick','duration_ticks'].some(k=>row[k]!==original[k]))tr.className='changed';
   for(const field of ['midi_note','start_tick','duration_ticks','rationale']){const td=document.createElement('td'),input=document.createElement(field==='rationale'?'textarea':'input');input.value=row[field];input.setAttribute('aria-label',row.note_id+' '+field.replaceAll('_',' '));
    if(field!=='rationale'){input.type='number';input.min=field==='duration_ticks'?'1':'0';input.max=field==='midi_note'?'127':String(session.duration_ticks);input.step='1';}else input.maxLength=4096;
    input.addEventListener('change',()=>{remember();row[field]=field==='rationale'?input.value:input.value.trim()===''?NaN:Number(input.value);update();render();});td.append(input);tr.append(td);
   }$('rows').append(tr);
  }
  const underlay=session.lead_sheet?.melody_part_id===selected?session.lead_sheet.underlay:[];$('underlay').textContent=underlay.length?`${underlay.length} source-bound lyric spans retained. Lyric text and underlay are unchanged.`:'No lyric underlay edited in this workflow.';
  $('page').textContent=`Page ${page+1} of ${pages}`;$('previous').disabled=page===0;$('next').disabled=page===pages-1;
 }
 $('part').addEventListener('change',()=>{page=0;render();});$('previous').addEventListener('click',()=>{page--;render();});$('next').addEventListener('click',()=>{page++;render();});
 $('candidate').addEventListener('change',()=>{remember();edit.candidate_model_id=$('candidate').value;update();});
 $('undo').addEventListener('click',()=>{if(!history.length)return;edit=history.pop();$('candidate').value=edit.candidate_model_id;render();update();});
 $('reset').addEventListener('click',()=>{remember();edit=structuredClone(session.edit_request);$('candidate').value=edit.candidate_model_id;render();update();});
 $('save').addEventListener('click',()=>{if(!NoteEditor.valid(session,edit))return;const url=URL.createObjectURL(new Blob([JSON.stringify(edit)],{type:'application/json'})),a=document.createElement('a');a.href=url;a.download='music-note-edit.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
 $('open').addEventListener('click',()=>$('file').click());$('file').addEventListener('change',async()=>{try{const file=$('file').files[0];if(!file)return;if(file.size>2*1024*1024)throw Error('Draft exceeds the 2 MiB limit');const candidate=JSON.parse(await file.text());if(!NoteEditor.valid(session,candidate))throw Error('Draft does not match this model or contains invalid notes');remember();edit=candidate;$('candidate').value=edit.candidate_model_id;render();update();}catch(error){message(error.message);}finally{$('file').value='';}});
 render();update();
})().catch(error=>{document.getElementById('status').textContent=error.message;});
