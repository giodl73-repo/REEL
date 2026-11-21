'use strict';
const $=id=>document.getElementById(id), fmt=n=>n.toLocaleString('en-US');
let session, csvRows=[];
const option=(value,label)=>{const o=document.createElement('option');o.value=value;o.textContent=label;return o};
function table(headers,rows,container){
  const table=document.createElement('table'),head=table.createTHead().insertRow();
  for(const label of headers){const th=document.createElement('th');th.textContent=label;th.scope='col';head.append(th)}
  const body=table.createTBody();for(const values of rows){const tr=body.insertRow();values.forEach((v,i)=>{const td=tr.insertCell();td.textContent=typeof v==='number'?fmt(v):v;if(i===0)td.style.fontWeight='600'})}
  container.replaceChildren(table);if(!rows.length){const p=document.createElement('p');p.className='empty';p.textContent='No speaking turns match these filters.';container.append(p)}
}
function episodes(){
  const previous=$('episode').value;$('episode').replaceChildren(option('','All episodes'));
  for(const e of session.request.episodes.filter(e=>!$('season').value||String(e.season)===$('season').value))$('episode').append(option(e.id,e.id));
  if([...$('episode').options].some(o=>o.value===previous))$('episode').value=previous;
}
function render(){
  const language=$('language').value, season=$('season').value, episode=$('episode').value, search=$('search').value.toLocaleUpperCase(), role=$('roles').value,metric=$('metric').value,pivot=$('pivot').value;
  const rows=session.turns.filter(t=>t.language===language&&(!season||String(t.season)===season)&&(!episode||t.episode===episode)&&(!search||t.character.toLocaleUpperCase().includes(search))&&(role==='all'||(role==='generic')===t.generic_role));
  $('lines').textContent=fmt(rows.reduce((s,t)=>s+t.dialogue_lines,0));$('turns').textContent=fmt(rows.length);$('words').textContent=fmt(rows.reduce((s,t)=>s+t.spoken_words,0));$('characters').textContent=fmt(new Set(rows.map(t=>t.character_id)).size);
  const column=t=>pivot==='season'?`Season ${t.season}`:pivot==='episode'?t.episode:'Overall';
  const columns=[...new Set(rows.map(column))].sort((a,b)=>a.localeCompare(b,undefined,{numeric:true})), grouped=new Map(),totalWords=rows.reduce((s,t)=>s+t.spoken_words,0);
  for(const t of rows){if(!grouped.has(t.character_id))grouped.set(t.character_id,{name:t.character,values:new Map(),words:0,episodes:new Set()});const group=grouped.get(t.character_id),key=column(t);group.values.set(key,(group.values.get(key)||0)+(metric==='turns'?1:t[metric]));group.words+=t.spoken_words;group.episodes.add(t.episode)}
  const ordered=[...grouped.values()].sort((a,b)=>[...b.values.values()].reduce((s,n)=>s+n,0)-[...a.values.values()].reduce((s,n)=>s+n,0)||a.name.localeCompare(b.name));
  const headers=['Character',...columns,'Total','Word share','Speaking episodes'];
  const values=ordered.map(g=>[g.name,...columns.map(c=>g.values.get(c)||0),[...g.values.values()].reduce((s,n)=>s+n,0),`${totalWords?(100*g.words/totalWords).toFixed(1):'0.0'}%`,g.episodes.size]);
  table(headers,values,$('matrix'));csvRows=[headers,...values];
  $('scope').textContent=`${session.request.title} · ${language==='es'?'Spanish':language==='en'?'English':language} · ${season?`Season ${season}`:'All seasons'}${episode?` · ${episode}`:''} · ${$('metric').selectedOptions[0].textContent}. Sorted by total. Shares use the active filters.`;
  const totals=new Map();for(const t of rows){if(!totals.has(t.episode))totals.set(t.episode,{season:t.season,lines:0,turns:0,words:0,roles:new Set()});const a=totals.get(t.episode);a.lines+=t.dialogue_lines;a.turns++;a.words+=t.spoken_words;a.roles.add(t.character_id)}
  table(['Episode','Season','Source lines','Turns','Words','Speaking roles'],[...totals].sort(([a],[b])=>a.localeCompare(b,undefined,{numeric:true})).map(([id,t])=>[id,t.season,t.lines,t.turns,t.words,t.roles.size]),$('episodeTotals'));
}
async function init(){
  const response=await fetch('session.json');if(!response.ok)throw Error('Could not load review session');session=await response.json();if(session.schema!=='reel.screenplay-session.v1')throw Error('Unsupported review session');
  const r=session.request;$('title').textContent=r.title;$('subtitle').textContent=`${r.revision} · ${r.episodes.length} episodes · Dialogue, ensemble and bilingual structure. ${r.notice}`;
  $('show').append(option(r.show_id,r.title));for(const lang of r.languages)$('language').append(option(lang,lang==='en'?'English':lang==='es'?'Spanish':lang));
  for(const s of [...new Set(r.episodes.map(e=>e.season))].sort((a,b)=>a-b))$('season').append(option(String(s),`Season ${s}`));episodes();
  for(const [name,note] of Object.entries(r.visual_roles)){const p=document.createElement('p'),strong=document.createElement('strong');strong.textContent=name+': ';p.append(strong,document.createTextNode(note));$('visual').append(p)}
  $('provenance').textContent=`Source commit: ${r.source_commit}\nScene and canonical cue order checked between languages: ${session.bilingual_structure_checked}\nThis checks order and declared identities, not translation meaning.\nEach source was captured against its declared SHA-256 and byte size.\n\n`+r.episodes.flatMap(e=>e.sources.map(s=>`${e.id} ${s.language}: ${s.path}\nSHA256 ${s.sha256} · ${s.bytes} bytes`)).join('\n');
  for(const id of ['language','episode','metric','pivot','roles'])$(id).addEventListener('change',render);$('search').addEventListener('input',render);$('season').addEventListener('change',()=>{episodes();render()});
  $('reset').addEventListener('click',()=>{for(const id of ['season','search'])$(id).value='';$('roles').value='all';$('metric').value='dialogue_lines';$('pivot').value='season';episodes();$('episode').value='';render()});
  $('csv').addEventListener('click',()=>{const quote=x=>'"'+String(x).replaceAll('"','""')+'"';const safe=x=>typeof x==='string'&&/^[=+@-]/.test(x)?"'"+x:x;const content='\ufeff'+csvRows.map(row=>row.map(x=>quote(safe(x))).join(',')).join('\r\n');const url=URL.createObjectURL(new Blob([content],{type:'text/csv;charset=utf-8'})),a=document.createElement('a');a.href=url;a.download=`${r.show_id}-${$('language').value}-dialogue.csv`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000)});
  render();
}
init().catch(e=>{$('status').textContent=e.message;});
