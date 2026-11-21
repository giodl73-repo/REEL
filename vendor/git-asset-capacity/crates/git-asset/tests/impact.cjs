'use strict';
// Synthetic controller scenarios; no browser, server or project/cache IO.
const {test}=require('node:test'),assert=require('node:assert/strict');
const {readFileSync}=require('node:fs'),{join}=require('node:path'),vm=require('node:vm');
const source=readFileSync(join(__dirname,'../src/catalogue/web/impact.js'),'utf8');
const reference=id=>({id,object:{sha256:'1'.repeat(64),bytes:12}});
function report(names=['scene','render']){
 return {project:'p',commit:'a'.repeat(40),basis_sha256:'b'.repeat(64),status:'complete',paging:{next_offset:null},
  data:{target:reference('input'),total_known:names.length,gap_count:0,gaps:[],unadmitted_definition_count:0,
   items:names.map(id=>({asset:reference(id),class:'output',distance:1,path:[{from:reference(id),target:reference('input'),kind:'composition',pointer:'/inputs/0'}],navigation:{project:'p',id,title:id,category:'items',selected:null,status:{}}}))}};
}
function fixture(){
 const element=(tag='',className='',textContent='')=>({tag,className,textContent,children:[],attributes:{},hidden:false,
  append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;},setAttribute(k,v){this.attributes[k]=v;},focus(){this.focused=true;},set innerHTML(_){throw Error('Unsafe HTML insertion');}});
 const elements=new Map(),$=id=>{if(!elements.has(id))elements.set(id,element());return elements.get(id);};
 const asset={project:'p',id:'input',details:{commit:'a'.repeat(40),record:reference('input')}},state={selected:asset,tab:'Impact'};
 const requests=[],inspections=[];let context;
 const flatten=e=>[e,...e.children.flatMap(flatten)];
 const redraw=()=>{$('detail').replaceChildren();context.drawImpact(state.selected,$('detail'));};
 context=vm.createContext({state,$,node:element,fmt:new Intl.NumberFormat(),document:{getElementById:id=>flatten($('detail')).find(e=>e.id===id)},
  drawDetail:redraw,inspect:async(card,tab='Overview')=>{inspections.push({card,tab});state.selected=card;state.tab=tab;},
  api:async(path,params)=>{requests.push({path,params});return report();}});
 vm.runInContext(source,context);redraw();
 return {asset,state,context,$,requests,inspections,redraw,nodes:()=>flatten($('detail')),text:()=>flatten($('detail')).map(e=>e.textContent).join('\n'),trace:more=>context.traceImpact(asset,asset.impactView,more)};
}

test('Impact opens without IO and exposes five labelled relationship choices',()=>{
 const f=fixture();assert.equal(f.requests.length,0);const checks=f.nodes().filter(e=>e.type==='checkbox');
 assert.equal(checks.length,5);assert.deepEqual(checks.map(e=>e.checked),[true,true,true,false,false]);
 assert.match(f.text(),/Relationship kinds/);assert.match(f.text(),/trace consumers to see paths/);
 assert.ok(f.nodes().some(e=>e.textContent==='Trace consumers'&&e.tag==='button'));
});
test('Trace uses explicit project/snapshot/kinds and renders exact expandable paths',async()=>{
 const f=fixture();await f.trace();assert.equal(f.requests[0].path,'/api/impact');
 assert.equal(f.requests[0].params.snapshot,'a'.repeat(40));assert.equal(f.requests[0].params.project,'p');
 assert.equal(f.requests[0].params.kinds,'production,composition,publication');
 assert.match(f.text(),/2 known consumers/);assert.match(f.text(),/Declared relationship coverage resolved/);
 assert.match(f.text(),/scene → input/);assert.match(f.text(),/composition · \/inputs\/0/);
 assert.ok(f.nodes().some(e=>e.tag==='summary'&&e.textContent==='View exact dependency path (1 step)'));
 assert.ok(f.nodes().some(e=>e.attributes['aria-live']==='polite'));
 assert.equal(f.nodes().find(e=>e.id==='impact-feedback').focused,true);
});
test('Changed filters clear results, preserve checkbox focus and require another explicit trace',async()=>{
 const f=fixture();await f.trace();const check=f.nodes().find(e=>e.id==='impact-production');check.checked=false;check.onchange();
 assert.equal(f.requests.length,1);assert.equal(f.asset.impactView.rows.length,0);assert.equal(f.asset.impactView.report,null);
 assert.equal(f.nodes().find(e=>e.id==='impact-production').focused,true);
 await f.trace();assert.equal(f.requests[1].params.kinds,'composition,publication');
 f.nodes().filter(e=>e.type==='checkbox'&&e.checked).forEach(e=>{e.checked=false;e.onchange();});
 await f.trace();assert.match(f.text(),/Choose at least one relationship/);assert.equal(f.requests.length,2);
});
test('Partial zero results expose gaps and exact missing definition pins without absence claims',async()=>{
 const f=fixture(),r=report([]);r.status='partial';r.data.gap_count=101;r.data.gaps_truncated=true;
 r.data.gaps=[{at:reference('unresolved'),reason:'Unadmitted'}];r.data.unadmitted_definition_count=1;
 r.data.unadmitted_definitions=[{definition:reference('type/item'),registered_records:7}];f.context.api=async()=>r;
 await f.trace();assert.match(f.text(),/0 known consumers/);assert.match(f.text(),/Coverage incomplete/);
 assert.match(f.text(),/Unadmitted · unresolved/);assert.match(f.text(),/First 100 gaps shown/);
 assert.match(f.text(),/Relationship rules needed \(1 definition\)/);assert.match(f.text(),/7 registered records/);
 assert.match(f.text(),/1{64} · 12 B/);assert.match(f.text(),/No known consumers; unresolved relationships remain/);
});
test('Consumer paging fences basis and clears stale results after a refused page',async()=>{
 const f=fixture();let calls=0,params;f.context.api=async(_,p)=>{params=p;const r=report(calls++?['later']:['first']);r.data.total_known=26;r.paging.next_offset=calls===1?25:null;return r;};
 await f.trace();await f.trace(true);assert.equal(params.offset,25);assert.equal(params.basis,'b'.repeat(64));
 assert.deepEqual(Array.from(f.asset.impactView.rows,r=>r.asset.id),['first','later']);
 f.asset.impactView.next=25;f.context.api=async()=>{throw Error('stale_basis');};await f.trace(true);
 assert.equal(f.asset.impactView.rows.length,0);assert.equal(f.asset.impactView.report,null);assert.match(f.text(),/stale_basis/);
});
test('Late replies after changed filters or leaving the inspector cannot replace active results',async()=>{
 const f=fixture();let release;f.context.api=()=>new Promise(resolve=>{release=resolve;});const old=f.trace();
 const check=f.nodes().find(e=>e.id==='impact-evidence');check.checked=true;check.onchange();
 f.context.api=async()=>report(['new']);await f.trace();release(report(['old']));await old;
 assert.deepEqual(Array.from(f.asset.impactView.rows,r=>r.asset.id),['new']);
 f.context.api=()=>new Promise(resolve=>{release=resolve;});const departed=f.trace();f.state.tab='Overview';release(report());await departed;
 assert.equal(f.asset.impactView.report,null);assert.equal(f.asset.impactView.loading,false);
});
test('Historical pins and untrusted titles stay text; current consumers have a return-to-impact link',async()=>{
 const f=fixture(),r=report(['<img src=x onerror=alert(1)>']);r.data.items[0].navigation=null;f.context.api=async()=>r;
 await f.trace();assert.match(f.text(),/<img src=x/);assert.match(f.text(),/Historical revision/);
 assert.equal(f.nodes().filter(e=>e.tag==='button'&&e.className==='member-button').length,0);
 f.context.api=async()=>report(['current']);await f.trace();
 let returned=false;f.$('back-work').hidden=false;f.$('back-work').textContent='← Back to work';f.$('back-work').onclick=()=>{returned=true;};
 await f.nodes().find(e=>e.tag==='button'&&e.textContent==='current').onclick();
 assert.equal(f.inspections[0].card.id,'current');assert.equal(f.$('back-work').hidden,false);
 await f.$('back-work').onclick();assert.equal(f.inspections[1].card,f.asset);assert.equal(f.inspections[1].tab,'Impact');
 assert.equal(f.$('back-work').textContent,'← Back to work');f.$('back-work').onclick();assert.equal(returned,true);
});
test('Changed snapshot, target pin or paging basis refuses mixed results',async()=>{
 for(const change of ['snapshot','target','basis']){
  const f=fixture();await f.trace();if(change==='basis')f.asset.impactView.next=25;
  const r=report();if(change==='snapshot')r.commit='c'.repeat(40);if(change==='target')r.data.target.object.sha256='d'.repeat(64);if(change==='basis')r.basis_sha256='e'.repeat(64);
  f.context.api=async()=>r;await f.trace(change==='basis');assert.equal(f.asset.impactView.rows.length,0);assert.ok(f.asset.impactView.error);
 }
});
