'use strict';
const CelSorter = {
 context(session) {
  const schemas={'reel.cel-sorter-session.v1':'reel.cel-sorter-edit.v1','reel.cel-sorter-session.v2':'reel.cel-sorter-edit.v2','reel.cel-sorter-session.v3':'reel.cel-sorter-edit.v3','reel.cel-sorter-session.v4':'reel.cel-sorter-edit.v4'};
  if(!Object.hasOwn(schemas,session.schema))throw Error('Unsupported editor session');
  const mapped=session.schema!=='reel.cel-sorter-session.v1',scoped=session.schema==='reel.cel-sorter-session.v4';
  if(mapped?!/^[a-f0-9]{64}$/.test(session.expected_project||''):session.expected_project!==undefined)throw Error('Invalid project version');
  if(scoped?(typeof session.language!=='string'||!session.language.trim()||new TextEncoder().encode(session.language).length>64):session.language!==undefined)throw Error('Invalid language scope');
  return {schema:schemas[session.schema],...(mapped?{expected_project:session.expected_project}:{}),...(scoped?{language:session.language}:{})};
 },
 draft(session,rows,notes) {return {...this.context(session),scene_id:session.scene_id,expected_inputs:structuredClone(session.expected_inputs),rows:structuredClone(rows),notes,human_approval:false};},
 locksEqual(a,b) {return !!(a&&b&&!Array.isArray(a)&&!Array.isArray(b)&&typeof a==='object'&&typeof b==='object'&&Object.keys(a).length===Object.keys(b).length&&Object.keys(a).every(k=>Object.hasOwn(b,k)&&typeof a[k]==='string'&&a[k]===b[k]));},
 matches(session,request) {try{const context=this.context(session);return request.language===context.language&&request.schema===context.schema&&request.expected_project===context.expected_project&&request.scene_id===session.scene_id&&this.locksEqual(request.expected_inputs,session.expected_inputs);}catch{return false;}},
 key(session) {this.context(session);return 'reel-cel-sorter:'+(session.schema!=='reel.cel-sorter-session.v1'?session.schema.slice(-2)+':'+session.expected_project+':':'')+JSON.stringify(session.expected_inputs);},
 starts(rows) { let t=0; return rows.map(r=>{const s=t;t+=r.frames;return s;}); },
 clockNotice(session) {const state=session.delivery_clock?.state;return state==='mixed-native-rates'?'Recordings use different sample rates. Delivery needs recordings at one matching sample rate. You can keep editing this draft.':state==='profile-rate-mismatch'?'Recording and delivery sample rates differ. Choose matching recordings or a matching delivery profile before rendering. You can keep editing this draft.':'';},
 sampleMode(known) {return known[0]?.cue_id!==undefined;},
 groups(known) {const result=[];for(const row of known){let g=result.at(-1);if(!g||g.id!==row.cue_id){g={id:row.cue_id,rate:row.sample_rate,total:0};result.push(g);}g.total+=row.samples;}return result;},
 valid(rows, known, total, pictures=known.map(row=>row.picture_binding)) {
  if(!Array.isArray(rows)||!rows.length||rows.length>known.length||new Set(rows.map(r=>r.semantic_id)).size!==rows.length)return false;
  const sample=this.sampleMode(known),groups=sample?this.groups(known):[],sums=new Map();let previous=-1;
  for(const r of rows){const k=known.find(k=>k.semantic_id===r.semantic_id),q=sample?r.samples:r.frames;
   if(!k||!pictures.includes(r.picture_binding)||!Number.isSafeInteger(q)||q<1||(r.comment!==undefined&&typeof r.comment!=='string'))return false;
   if(sample){const index=groups.findIndex(g=>g.id===k.cue_id);if(r.cue_id!==k.cue_id||r.sample_rate!==k.sample_rate||!Number.isSafeInteger(r.sample_rate)||r.sample_rate<1||r.frames!==undefined||index<previous)return false;previous=index;sums.set(k.cue_id,(sums.get(k.cue_id)||0)+q);}
   else if(r.samples!==undefined)return false;
  }
  if(sample)return groups.every(g=>sums.has(g.id)&&Number.isSafeInteger(sums.get(g.id))&&(total==null||sums.get(g.id)===g.total));
  return total==null||rows.reduce((s,r)=>s+r.frames,0)===total;
 },
 fit(rows,total,known=rows,pictures=known.map(row=>row.picture_binding)) {
  if(!this.valid(rows,known,null,pictures))throw Error('Invalid cue sequence');
  const distribute=(part,budget,key)=>{if(part.length>budget)throw Error('Too many cels');const available=budget-part.length,sum=part.reduce((s,r)=>s+r[key],0);let previous=0,cumulative=0;return part.map(r=>{cumulative+=r[key];const next=Math.round(cumulative/sum*available),quantity=1+next-previous;previous=next;return {...r,[key]:quantity};});};
  if(!this.sampleMode(known))return distribute(rows,total,'frames');
  return this.groups(known).flatMap(g=>distribute(rows.filter(r=>r.cue_id===g.id),g.total,'samples'));
 },
 duration(row,fps) {return row.cue_id===undefined?row.frames/fps:row.samples/row.sample_rate;},
 recordings(session) {
  if(session.media_mode!=='native-recordings')return null;
  if(session.movie!=null||!Array.isArray(session.recordings)||!session.recordings.length||session.recordings.length>128)throw Error('Invalid native recordings');
  const cues=session.cues||[{cue_id:session.recordings[0].cue_id,samples:session.end_sample,sample_rate:session.sample_rate,start_seconds:0}];
  if(cues.length!==session.recordings.length)throw Error('Recording cue coverage differs');
  let offset=0;const ids=new Set();
  return session.recordings.map((r,i)=>{const c=cues[i];if(typeof r.cue_id!=='string'||!r.cue_id||ids.has(r.cue_id)||r.cue_id!==c.cue_id||r.samples!==c.samples||r.sample_rate!==c.sample_rate||!Number.isSafeInteger(r.samples)||r.samples<1||!Number.isSafeInteger(r.sample_rate)||r.sample_rate<1||r.start_seconds!==offset||c.start_seconds!==offset||!/^[a-f0-9]{64}$/.test(r.sha256)||r.src!=='media/'+r.sha256+'.wav'||!Number.isSafeInteger(r.bytes)||r.bytes<1)throw Error('Native recording does not match its cue clock');ids.add(r.cue_id);const result={...r,start_seconds:offset};offset+=r.samples/r.sample_rate;return result;});
 },
 recordingAt(recordings,time) {let i=0;for(let n=1;n<recordings.length;n++)if(recordings[n].start_seconds<=time)i=n;return {index:i,local:Math.max(0,Math.min(time-recordings[i].start_seconds,recordings[i].samples/recordings[i].sample_rate))};},
 timeStarts(rows,known,fps) {
  if(!this.sampleMode(known))return this.starts(rows).map(t=>t/fps);
  let offset=0;const clocks=new Map(this.groups(known).map(g=>{const value=offset;offset+=g.total/g.rate;return [g.id,value];})),elapsed=new Map();
  return rows.map(r=>{const value=clocks.get(r.cue_id)+(elapsed.get(r.cue_id)||0);elapsed.set(r.cue_id,(elapsed.get(r.cue_id)||0)+this.duration(r,fps));return value;});
 }

};
if(typeof module!=='undefined')module.exports=CelSorter;
if(typeof document!=='undefined') (async()=>{
const $=id=>document.getElementById(id),session=await fetch('session.json').then(r=>{if(!r.ok)throw Error('Could not load the sequence');return r.json();});
const fps=session.fps_numerator/session.fps_denominator,sample=CelSorter.sampleMode(session.rows),seconds=sample?session.total_seconds:session.end_sample/session.sample_rate,total=sample?seconds:Math.round(seconds*fps),key=CelSorter.key(session),original=structuredClone(session.rows),pictures=session.picture_choices||[...new Set(session.rows.map(row=>row.picture_binding))],history=[];
let rows=structuredClone(original),notes='',selected=0,drag=null,lastDisplayed=-1;
try{const saved=JSON.parse(localStorage.getItem(key));if(saved&&CelSorter.valid(saved.rows,original,null,pictures)){rows=saved.rows;notes=saved.notes||'';}}catch{}
$('language-scope').textContent=session.schema==='reel.cel-sorter-session.v4'?'Editing '+session.language+'. Other languages stay unchanged.':'';$('language-scope').hidden=session.schema!=='reel.cel-sorter-session.v4';
const clockNotice=CelSorter.clockNotice(session);$('delivery-clock').textContent=clockNotice;$('delivery-clock').hidden=!clockNotice;
if(session.media_mode!==undefined&&!['controls-only','native-recordings'].includes(session.media_mode))throw Error('Unsupported media mode');
const recordings=CelSorter.recordings(session);let recordingIndex=0,loadGeneration=0,pendingTime=null;
const media=session.media_mode!=='controls-only';
$('media-notice').hidden=media&&!recordings;$('media-notice').textContent=recordings?'Primary recordings preview. The audio controls show the current cue; the position slider spans the sequence. Cue changes may pause briefly. Music, effects and final rendering are not included.':media?'':'Controls-only session: media previews and playback are unavailable. Recording clocks and source locks remain enforced.';
for(const id of ['preview','audio','seek'])$(id).hidden=!media;
$('title').textContent=session.title||'Cel sorter';if(media)$('audio').src=recordings?recordings[0].src:session.movie;$('seek').max=seconds;$('notes').value=notes;
function status(message,error=false){$('status').textContent=message;$('status').className=error?'error':'';}
function position(){return recordings?recordings[recordingIndex].start_seconds+(pendingTime??$('audio').currentTime??0):$('audio').currentTime||0;}
function seek(time,play=false){const a=$('audio');if(!recordings){a.currentTime=time;return;}const target=CelSorter.recordingAt(recordings,time),generation=++loadGeneration,resume=play||!a.paused;pendingTime=target.local;
 const ready=()=>{if(generation!==loadGeneration)return;try{a.currentTime=target.local;pendingTime=null;if(resume)a.play().catch(()=>status('Press Play to continue the recordings.',true));}catch{status('Recording could not be positioned.',true);}};
 if(target.index!==recordingIndex){a.pause();recordingIndex=target.index;a.src=recordings[recordingIndex].src;a.addEventListener('loadedmetadata',ready,{once:true});a.load();}else if(a.readyState>=1)ready();else a.addEventListener('loadedmetadata',ready,{once:true});
}
if(recordings){$('audio').addEventListener('ended',()=>{if(recordingIndex+1<recordings.length)seek(recordings[recordingIndex+1].start_seconds,true);});$('audio').addEventListener('error',()=>{pendingTime=null;status('This primary recording could not be played. Its original cue clock is unchanged.',true);});$('audio').addEventListener('loadedmetadata',()=>{const r=recordings[recordingIndex];if(Math.abs($('audio').duration-r.samples/r.sample_rate)>Math.max(.05,1/r.sample_rate))status('Recording duration differs from its declared cue clock. Check the source before review.',true);});}
function persist(){try{localStorage.setItem(key,JSON.stringify({rows,notes:$('notes').value}));status('Changes and comments saved in this browser.');}catch{status('Could not save in this browser. Download the draft to keep your changes.',true);}}
function before(){history.push(structuredClone(rows));if(history.length>100)history.shift();}
function change(fn){const prior=structuredClone(rows);fn();if(!CelSorter.valid(rows,original,null,pictures)){rows=prior;status('Keep at least one cel in each cue, in its original cue group.',true);render();return;}history.push(prior);if(history.length>100)history.shift();selected=Math.min(selected,rows.length-1);persist();render();show(selected);}
function show(i){if(!rows[i])return;selected=i;if(media)$('preview').src=session.images[rows[i].picture_binding];else $('clock').textContent='Cel '+(i+1)+' of '+rows.length+' · '+CelSorter.timeStarts(rows,original,fps)[i].toFixed(2)+' s · media not loaded';$('preview').alt='Cel '+(i+1)+' · '+rows[i].semantic_id;document.querySelectorAll('.card').forEach((c,n)=>c.classList.toggle('selected',n===i));}
function go(i){if(i<0||i>=rows.length)return;if(media)seek(CelSorter.timeStarts(rows,original,fps)[i]);show(i);}
function render(){const host=$('filmstrip');host.replaceChildren();const starts=CelSorter.timeStarts(rows,original,fps);$('save').disabled=!CelSorter.valid(rows,original,total,pictures);$('undo').disabled=!history.length;
 rows.forEach((r,i)=>{const card=document.createElement('article');card.className='card';card.draggable=true;card.dataset.index=i;card.addEventListener('dragstart',e=>{if(['INPUT','SELECT','BUTTON','TEXTAREA'].includes(e.target.tagName)){e.preventDefault();return;}drag=i;e.dataTransfer.effectAllowed='move';e.dataTransfer.setData('text/plain',String(i));card.classList.add('dragging');});card.addEventListener('dragend',()=>{drag=null;card.classList.remove('dragging');});card.addEventListener('dragover',e=>e.preventDefault());card.addEventListener('drop',e=>{e.preventDefault();if(drag===null||drag===i)return;const from=drag;change(()=>rows.splice(i,0,rows.splice(from,1)[0]));});
 const image=document.createElement('img');if(media)image.src=session.images[r.picture_binding];else{image.hidden=true;card.classList.add('controls-only');}image.className='thumb';image.loading='lazy';image.alt='Cel '+(i+1);image.addEventListener('click',()=>go(i));card.append(image);const detail=document.createElement('div');detail.className='detail';const label=document.createElement('strong');label.textContent=String(i+1).padStart(2,'0')+' · '+(sample?r.cue_id+' · ':'')+r.semantic_id;detail.append(label);const meta=document.createElement('div');meta.className='meta';meta.textContent=starts[i].toFixed(2)+' s → '+(starts[i]+CelSorter.duration(r,fps)).toFixed(2)+' s';detail.append(meta);
 const duration=document.createElement('label');duration.textContent='Hold (seconds) ';const input=document.createElement('input');input.type='number';const rate=sample?r.sample_rate:fps;input.min=String(1/rate);input.step='any';input.value=String(CelSorter.duration(r,fps));input.setAttribute('aria-label','Hold duration for cel '+(i+1));input.addEventListener('change',()=>{const quantity=Math.round(Number(input.value)*rate);if(!Number.isSafeInteger(quantity)||quantity<1){input.value=String(CelSorter.duration(r,fps));return;}change(()=>rows[i][sample?'samples':'frames']=quantity);});duration.append(input);detail.append(duration);
 const select=document.createElement('select');select.setAttribute('aria-label','Image for cel '+(i+1));pictures.forEach((binding,n)=>{const o=document.createElement('option');o.value=binding;o.textContent='Image '+String(n+1).padStart(2,'0')+' · '+binding;o.selected=binding===r.picture_binding;select.append(o);});select.addEventListener('change',()=>change(()=>rows[i].picture_binding=select.value));detail.append(select);
 const buttons=document.createElement('div');buttons.className='toolbar';for(const [label,fn,disabled] of [['↑',()=>change(()=>[rows[i-1],rows[i]]=[rows[i],rows[i-1]]),i===0||(sample&&rows[i-1].cue_id!==r.cue_id)],['↓',()=>change(()=>[rows[i+1],rows[i]]=[rows[i],rows[i+1]]),i===rows.length-1||(sample&&rows[i+1].cue_id!==r.cue_id)],['View',()=>go(i),false],['Remove',()=>change(()=>rows.splice(i,1)),sample?rows.filter(x=>x.cue_id===r.cue_id).length===1:rows.length===1]]){const b=document.createElement('button');b.textContent=label;b.disabled=disabled;b.addEventListener('click',fn);buttons.append(b);}detail.append(buttons);const commentLabel=document.createElement('label');commentLabel.className='cel-comment';commentLabel.textContent='Comment';const comment=document.createElement('textarea');comment.value=r.comment||'';comment.placeholder='What would you change in this cel?';comment.setAttribute('aria-label','Comment for cel '+(i+1)+' · '+r.semantic_id);comment.addEventListener('focus',before);comment.addEventListener('input',()=>{rows[i].comment=comment.value;persist();$('undo').disabled=!history.length;});commentLabel.append(comment);detail.append(commentLabel);card.append(detail);host.append(card);});
 if(!CelSorter.valid(rows,original,total,pictures))status('Holds must fill each original recording exactly. Adjust holds or use Fit holds to recordings.',true);show(selected);}
$('notes').addEventListener('input',persist);$('prev').onclick=()=>go(selected-1);$('next').onclick=()=>go(selected+1);$('fit').onclick=()=>change(()=>rows=CelSorter.fit(rows,total,original,pictures));$('undo').onclick=()=>{if(history.length){rows=history.pop();selected=Math.min(selected,rows.length-1);persist();render();}};$('reset').onclick=()=>{if(confirm('Reset the sequence and cel comments to the original?'))change(()=>rows=structuredClone(original));};$('seek').oninput=()=>{seek(Number($('seek').value));};
$('save').onclick=()=>{if(!CelSorter.valid(rows,original,total,pictures))return;const request=CelSorter.draft(session,rows,$('notes').value);const blob=new Blob([JSON.stringify(request,null,2)],{type:'application/json'}),a=document.createElement('a');if(blob.size>2097152){status('Draft exceeds the 2 MiB limit. Shorten comments or notes.',true);return;}const url=URL.createObjectURL(blob);a.href=url;a.download='cel-draft-'+new Date().toISOString().replace(/[:.]/g,'-')+'.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);status('Draft downloaded with all cel comments for the next review.');};
$('open').onclick=()=>$('file').click();$('file').onchange=async()=>{try{const file=$('file').files[0];if(!file||file.size>2097152)throw Error('Draft file is too large');const r=JSON.parse(await file.text());if(!CelSorter.matches(session,r)||!CelSorter.valid(r.rows,original,total,pictures))throw Error('This draft does not match this source version');change(()=>rows=r.rows);$('notes').value=r.notes||'';persist();}catch(e){status(e.message,true);}};
function formatTime(t){return Math.floor(t/60)+':'+String(Math.floor(t%60)).padStart(2,'0');}
function tick(){const t=position(),starts=CelSorter.timeStarts(rows,original,fps);let i=0;for(let n=0;n<starts.length;n++){if(starts[n]<=t)i=n;}if(i!==lastDisplayed){show(i);lastDisplayed=i;}$('seek').value=t;$('clock').textContent=formatTime(t)+' / '+formatTime(seconds)+' · cel '+(i+1)+' of '+rows.length;requestAnimationFrame(tick);}render();show(0);if(media)requestAnimationFrame(tick);
})().catch(e=>{document.getElementById('status').textContent=e.message;});
