'use strict';
// Synthetic frontend controller tests; no server, project data or browser IO.
const {test}=require('node:test'),assert=require('node:assert/strict');
const {readFileSync}=require('node:fs'),vm=require('node:vm');
const source=readFileSync(require('node:path').join(__dirname,'../src/catalogue/web/works.js'),'utf8');
function fixture(){
 const state={projects:[{id:'p',commit:'abc',collections:[
  {id:'one',label:'One',count_label:'Accepted one',count_state:'Accepted'},
  {id:'two',label:'Two',count_label:'Ready two',count_state:'Ready'}]}],
  project:'p',category:'',workCollections:[],mode:'works',workState:'',facet:'',q:'',generation:0,rows:[]};
 const element=(tag='',cls='',text='')=>({tag,className:cls,textContent:text,children:[],attributes:{},dataset:{},value:'',focus(){this.focused=true;},setAttribute(k,v){this.attributes[k]=v;},append(...items){this.children.push(...items);},replaceChildren(...items){this.children=items;}});
 const elements=new Map();const $=id=>{if(!elements.has(id))elements.set(id,element());return elements.get(id);};
 let requests=[];
 const context=vm.createContext({state,$,document:{querySelector:$,querySelectorAll:()=>$('.stats').children.filter(c=>c.className==='summary-filter')},notice(message){$('notice').textContent=message;},node:element,fmt:new Intl.NumberFormat(),
  bytes:n=>n+' B',projectLabel:id=>id,inspect(){},close(){},
  api:async(path,params)=>{requests.push({path,params});return {snapshots:{p:'abc'},items:[],total:0};}});
 vm.runInContext(source,context);
 vm.runInContext(readFileSync(require('node:path').join(__dirname,'../src/catalogue/web/shared.js'),'utf8'),context);
 vm.runInContext('drawNavigation=()=>{};load=()=>{}',context);
 return {state,context,$,requests};
}

test('work source can open Impact and preserves back-to-work after record inspection starts',()=>{
 const f=fixture(),work={id:'work'},card={id:'source'};let inspected,returned;
 f.state.selected={work};f.context.inspect=(item,tab)=>{inspected={item,tab};f.$('back-work').hidden=true;};
 f.context.inspectWork=item=>{returned=item;};f.context.openWorkMember(card,'Impact');
 assert.equal(inspected.item,card);assert.equal(inspected.tab,'Impact');
 assert.equal(f.$('back-work').hidden,false);f.$('back-work').onclick();assert.equal(returned,work);
});
test('ordinary, Ctrl and Meta clicks select, union and clear',()=>{
 const {state,context,$}=fixture();
 context.chooseCollection('one');assert.deepEqual(Array.from(state.workCollections),['one']);assert.equal(state.workState,'Accepted');
 context.chooseCollection('two',{ctrlKey:true});assert.deepEqual(Array.from(state.workCollections),['one','two']);assert.equal(state.workState,'');
 context.chooseCollection('one',{metaKey:true});assert.deepEqual(Array.from(state.workCollections),['two']);assert.equal(state.workState,'Ready');
 context.chooseCollection('two');assert.equal(state.workCollections.length,0);assert.equal(state.category,'');
 context.chooseCollection('one');context.chooseCollection('');assert.equal(state.workCollections.length,0);
 assert.equal($('search-scope').value,'collection');
});

test('rendered totals are accessible toggles and shared navigation supplies labelled selectors',()=>{
 const f=fixture(),p=f.state.projects[0];Object.assign(p,{label:'First',refs:[],categories:{photo:3},records:3,work_count:3});
 p.collections.forEach(c=>Object.assign(c,{count_value:2,count:3,states:{Accepted:2},facets:{},label:c.id}));
 f.context.Option=function(text,value){this.textContent=text;this.value=value;};
 f.context.navButton=(_,count,active,click)=>{const b=f.context.node('button');b.onclick=click;return b;};
 f.context.drawWorkNavigation();const totals=f.$('.stats').children;
 assert.equal(totals[0].tag,'button');assert.equal(totals[0].attributes['aria-pressed'],'false');
 f.context.drawNavigation=()=>f.context.drawWorkNavigation();
 totals[0].onclick({currentTarget:totals[0]});assert.equal(f.$('.stats').children[0].attributes['aria-pressed'],'true');
 assert.equal(f.$('.stats').children[0].focused,true);
 f.$('.stats').children[1].onclick({ctrlKey:true});assert.equal(f.state.workCollections.length,2);
 f.state.projects.push({id:'q',label:'Second',commit:'def',categories:{photo:2},work_count:2});
 vm.runInContext("sharedView.left='p';sharedView.right='q'",f.context);f.context.drawSharedNavigation();
 const controls=f.$('work-controls').children;
 assert.equal(controls[0].children[0].attributes['aria-label'],'First comparison project');
 assert.equal(controls[1].children[0].attributes['aria-label'],'Second comparison project');
 assert.equal(controls[2].children[0].attributes['aria-label'],'Shared asset category');
 assert.equal(f.$('.toolbar').hidden,true);
});

test('shared view compares exact pair, links references and refuses stale snapshots',async()=>{
 const f=fixture();f.state.projects.push({id:'q',commit:'def'});
 vm.runInContext("sharedView.left='p';sharedView.right='q';sharedView.category='photo'",f.context);
 const row={object:{sha256:'a'.repeat(64),bytes:7},left_total:1,right_total:1,left:[{asset:'a',definition:'d',category:'photo'}],right:[{asset:'b',definition:'d',category:'photo'}]};
 let params;f.context.api=async(_,p)=>{params=p;return {coverage:[{project:'p',commit:'abc',matched_records:3,without_selected_object:1},{project:'q',commit:'def',matched_records:2,without_selected_object:0}],items:[row],shared_objects:1,shared_unique_bytes:7};};
 await f.context.loadShared();assert.equal(params.left,'p');assert.equal(params.right,'q');assert.equal(params.category,'photo');
 assert.match(f.$('result-count').textContent,/1 unique shared objects/);assert.equal(f.$('more').hidden,true);
 let inspected;f.context.inspect=asset=>{inspected=asset;};const button=f.context.sharedReference('q',row.right[0]);button.onclick();assert.equal(inspected.id,'b');assert.equal(inspected.project,'q');
 f.state.projects[1].commit='changed';await f.context.loadShared();assert.match(f.$('notice').textContent,/snapshot changed/);assert.equal(f.state.rows.length,0);
 vm.runInContext("sharedView.right='p'",f.context);await f.context.loadShared();assert.match(f.$('notice').textContent,/two different/);
});

test('shared zero result is explicitly scoped, and late replies do not redraw',async()=>{
 const f=fixture();f.state.projects.push({id:'q',commit:'def'});vm.runInContext("sharedView.left='p';sharedView.right='q'",f.context);
 const report={coverage:[],items:[],shared_objects:0,shared_unique_bytes:0};f.context.api=async()=>report;
 await f.context.loadShared();assert.match(f.$('assets').children[0].textContent,/Unindexed files have not been compared/);
 let resolve;f.context.api=()=>new Promise(r=>resolve=r);const pending=f.context.loadShared();f.state.generation++;resolve(report);await pending;
 assert.equal(f.$('assets').children[0].textContent,'Comparing recorded identities…');
});
test('union request retains query and pages; stale responses cannot redraw',async()=>{
 const f=fixture();f.context.chooseCollection('one');f.context.chooseCollection('two',{ctrlKey:true});
 f.state.q='needle';await f.context.loadWorks();
 assert.equal(f.requests[0].params.collections,'one,two');assert.equal(f.requests[0].params.q,'needle');
 assert.equal(f.requests[0].params.state,undefined);
 f.state.offset=100;await f.context.loadWorks(true);assert.equal(f.requests[1].params.offset,100);
 let resolve;f.context.api=()=>new Promise(r=>resolve=r);const pending=f.context.loadWorks();
 f.state.generation++;resolve({snapshots:{p:'old'},items:[{}],total:1});await pending;
 assert.equal(f.state.rows.length,0);
});
