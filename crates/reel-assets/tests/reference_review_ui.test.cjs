const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const script = fs.readFileSync(path.join(__dirname, '../src/reference_review.js'), 'utf8');
class Element {
  constructor() { this.children=[]; this.events={}; this.value=''; this.hidden=false; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children=children; }
  addEventListener(name,handler) { this.events[name]=handler; }
}
function fixture(extra={}) {
  const graph={project_id:'independent-project',registry_generation:7,scope:'Scoped; no removal authorization',nodes:{
    inventory:{kind:'document',schema:'inventory.v1',status:'inspected',details:{}},
    media:{kind:'asset',schema:null,status:'present-size-only',details:{bytes:42}},
    version:{kind:'version',schema:null,status:'registered',details:{}}},
    edges:[{from:'inventory',to:'media',kind:'inventory-member',field:'verified_assets'},
      {from:'version',to:'media',kind:'retains-asset',field:'payloads'}],
    findings:[{node:'media',code:'test-hold',detail:'Unverified media'}],...extra};
  const nodes=new Map();
  const document={getElementById(id){if(!nodes.has(id))nodes.set(id,new Element());return nodes.get(id);},createElement(){return new Element();}};
  document.getElementById('data').textContent=JSON.stringify(graph);
  vm.runInNewContext(script,{document});
  return {get:id=>document.getElementById(id)};
}
test('search and typed relationship filters preserve membership semantics',()=>{
  const {get}=fixture(); assert.equal(get('nodes').children.length,3);
  get('kind').value='inventory-member';get('kind').events.change();assert.equal(get('nodes').children.length,2);
  get('search').value='media';get('search').events.input();assert.equal(get('nodes').children.length,1);
  get('nodes').children[0].events.click();
  assert.match(get('incoming').children[0].children[1].textContent,/does not establish a build dependency/);
  assert.match(get('findings').textContent,/test-hold/);
  assert.match(get('meaning').textContent,/does not authorize removal/);
});
test('relationship navigation works and hostile labels remain literal text',()=>{
  const hostile='</script><img src=https://example.invalid>';
  const {get}=fixture({project_id:hostile});
  assert.match(get('summary').textContent,/<img src=/);
  get('nodes').children[1].events.click();
  get('incoming').children[0].children[0].events.click();
  assert.equal(get('selection').textContent,'inventory');
  assert.equal(get('incoming').textContent,'None recorded in this scoped inspection.');
});
test('large graphs have bounded initial node rendering and expandable results',()=>{
  const nodes=Object.fromEntries(Array.from({length:430},(_,i)=>['node'+i,{kind:'asset',status:'present',schema:null,details:{}}]));
  const {get}=fixture({nodes,edges:[],findings:[]});
  assert.equal(get('nodes').children.length,200);assert.equal(get('more').hidden,false);
  get('more').events.click();assert.equal(get('nodes').children.length,400);
  get('more').events.click();assert.equal(get('nodes').children.length,430);assert.equal(get('more').hidden,true);
});
test('native required input links are distinct from inventory observations and show snapshot generation',()=>{
  const {get}=fixture({edges:[{from:'version',to:'media',kind:'requires-asset',field:'selected_inputs/picture'}]});
  assert.match(get('summary').textContent,/registry snapshot 7/);
  get('nodes').children[1].events.click();
  assert.match(get('incoming').children[0].children[1].textContent,/Required selected input returned by the native scene resolver/);
  assert.match(get('incoming').children[0].children[1].textContent,/size-only/);
});

test('authority and delivery links preserve evidence and release boundaries',()=>{
  for (const [kind,meaning] of [['source-authority',/does not establish authorship, consent or approval/],['delivers-version',/not a build dependency or current selection/],['delivers-asset',/no remote readback or release permission/],['delivery-evidence',/no destination was accessed/]]) {
    const {get}=fixture({edges:[{from:'version',to:'media',kind,field:'record'}]});
    get('nodes').children[1].events.click();
    assert.match(get('incoming').children[0].children[1].textContent,meaning);
  }
});
