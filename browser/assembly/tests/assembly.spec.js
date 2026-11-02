import {test,expect} from '@playwright/test';
import fs from 'node:fs';
const graph=JSON.parse(fs.readFileSync(new URL('../fixtures/graph.json',import.meta.url),'utf8'));
const pointer=fs.readFileSync(new URL('../fixtures/pointer.json',import.meta.url),'utf8');
async function ready(page){await page.goto('./');await expect(page.locator('#status')).toContainText('Ready.');await page.locator('#input').fill(JSON.stringify(graph));}
async function inspect(page){await page.locator('#inspect').click();await expect(page.locator('#status')).toContainText(/Graph inspected|Input was rejected/);return JSON.parse(await page.locator('#report').textContent());}

test('real worker preserves native unselected omissions, lock observations and presentation target',async({page})=>{
 await ready(page);let r=await inspect(page);expect(r.ok).toBe(true);expect(r.graph_valid).toBe(true);expect(r.closure.node_ids).toEqual(['episode','scene']);expect(r.closure.selected_assets.length).toBe(2);
 expect(r.declared_lock_matches_computed).toBe(false);await expect(page.locator('#summary')).toContainText('scene.vfx / vfx: unselected');await expect(page.locator('#summary')).toContainText('scene.score / score: explicit-silence');
 const timings=await page.locator('#status').evaluate(node=>({measurements:{...node.dataset},user_agent:navigator.userAgent,hardware_concurrency:navigator.hardwareConcurrency,fixture:{nodes:2,slots:4,events:1},scope:'Descriptive browser/worker/message sample; no benchmark or production-readiness claim.'}));
 for(const field of ['startupMs','workerInitializationMs','queryRoundtripMs','workerComputationMs'])expect(Number.isFinite(Number(timings.measurements[field]))).toBe(true);
 fs.writeFileSync('.cache/worker-timings.json',JSON.stringify(timings,null,2));
 await page.locator('details').filter({has:page.locator('#pointer')}).locator('summary').click();await page.locator('#pointer').fill(pointer);r=await inspect(page);expect(r.pointer_matches_supplied_graph_lock).toBe(true);expect(r.declared_lock_matches_computed).toBe(false);expect(r.selected.closure).toEqual(r.closure);
 await page.locator('#kind').selectOption('presentation');await page.locator('#target').fill('review');r=await inspect(page);expect(r.presentation.target_id).toBe('review');expect(r.closure.target).toBe('episode');
 const mismatch=JSON.parse(pointer);mismatch.selected_lock.sha256='f'.repeat(64);await page.locator('#pointer').fill(JSON.stringify(mismatch));r=await inspect(page);expect(r.ok).toBe(false);expect(r.graph_valid).toBe(true);expect(r.stage).toBe('native pointer matching');
});

test('WASM exactly matches every native request and existing production CLI controls',async({page})=>{
 await page.goto('./');const cases=JSON.parse(fs.readFileSync('.cache/native-cases.json','utf8'));const controls=JSON.parse(fs.readFileSync('.cache/original-cli-controls.json','utf8'));
 const actual=await page.evaluate(async({cases,controls})=>{const m=await import('./pkg/reel_assembly_workbench.js');await m.default();const run=r=>m.inspect_assembly(r.graph_text,r.target,r.kind,r.pointer_text);return {cases:cases.map(c=>run(c.request)),controls:controls.map(c=>JSON.parse(run(c.request)))};},{cases,controls});
 expect(actual.cases).toEqual(cases.map(c=>c.output));
 for(let i=0;i<controls.length;i++){const c=controls[i],r=actual.controls[i];if(c.native_exit_code===0)expect(r.selected).toEqual(c.expected_selected);else{expect(r.ok).toBe(false);expect(c.native_stderr).toContain(r.error);}}
 fs.writeFileSync('.cache/wasm-parity.json',JSON.stringify({exact_native_responses:cases.length,original_cli_controls:controls.length},null,2));
});

test('cyclic target fails after structural validation while an unrelated cycle stays out of scope',async({page})=>{
 await ready(page);const v=structuredClone(graph);v.nodes.push({id:'unused-cycle',inputs:['unused-cycle'],slots:[],events:[]});await page.locator('#input').fill(JSON.stringify(v));let r=await inspect(page);expect(r.ok).toBe(true);
 v.nodes[0].inputs=['episode'];await page.locator('#input').fill(JSON.stringify(v));r=await inspect(page);expect(r.graph_valid).toBe(true);expect(r.ok).toBe(false);expect(r.stage).toBe('native node closure');expect(r.error).toContain('cycle');
});

test('changing pointer or target fences older worker and file-read callbacks',async({page})=>{
 await page.addInitScript(()=>{window.workers=[];window.Worker=class{constructor(){window.workers.push(this);setTimeout(()=>this.onmessage?.({data:{type:'ready'}}),0);}postMessage(data){this.request=data;}terminate(){}};const original=Blob.prototype.arrayBuffer;Blob.prototype.arrayBuffer=function(){return new Promise(resolve=>setTimeout(()=>resolve(original.call(this)),700));};});
 await ready(page);await page.locator('#inspect').click();await page.locator('#target').fill('scene');await expect(page.locator('#status')).toContainText('Ready.');
 await page.evaluate(()=>{const w=window.workers[0];w.onmessage({data:{type:'result',id:w.request.id,text:JSON.stringify({ok:false,error:'OLD TARGET'})}});});await expect(page.locator('#download')).toBeDisabled();await expect(page.locator('#report')).toHaveText('No report yet.');
 await page.locator('#file').setInputFiles({name:'late.json',mimeType:'application/json',buffer:Buffer.from('{}')});await page.locator('details').filter({has:page.locator('#pointer')}).locator('summary').click();await page.locator('#pointer').fill(pointer);await page.waitForTimeout(900);await expect(page.locator('#input')).toHaveValue(JSON.stringify(graph));await expect(page.locator('#original')).toBeDisabled();
});

test('actual compiled WASM rejects memory growth above 64 MiB',async({page})=>{
 await page.goto('./');const result=await page.evaluate(async()=>{const m=await import('./pkg/reel_assembly_workbench.js');const wasm=await m.default();const before=wasm.memory.buffer.byteLength;wasm.memory.grow(1024-before/65536);let rejected=false;try{wasm.memory.grow(1);}catch{rejected=true;}return {rejected,bytes:wasm.memory.buffer.byteLength};});expect(result).toEqual({rejected:true,bytes:67108864});
});

test('admitted node/reference/byte bounds and whole refusals remain inspectable',async({page})=>{
 await ready(page);const v=structuredClone(graph);
 while(v.nodes.length<128){const id='chain-'+v.nodes.length;v.nodes.push({id,inputs:[v.nodes.at(-1).id],slots:[],events:[]});}
 await page.locator('#target').fill(v.nodes.at(-1).id);await page.locator('#input').fill(JSON.stringify(v));let r=await inspect(page);expect(r.ok).toBe(true);expect(r.closure.node_ids.length).toBe(128);
 const references=structuredClone(graph);references.nodes[1].inputs=Array(1019).fill('scene');await page.locator('#target').fill('episode');await page.locator('#input').fill(JSON.stringify(references));r=await inspect(page);expect(r.ok).toBe(true);
 references.nodes[1].inputs.push('scene');await page.locator('#input').fill(JSON.stringify(references));r=await inspect(page);expect(r.ok).toBe(false);expect(r.stage).toBe('workbench admission');
 const text=JSON.stringify(graph);await page.locator('#input').fill(text+' '.repeat(131072-Buffer.byteLength(text)));r=await inspect(page);expect(r.ok).toBe(true);expect(r.input_bytes).toBe(131072);
 await page.locator('#input').fill(text+' '.repeat(131073-Buffer.byteLength(text)));await page.locator('#inspect').click();await expect(page.locator('#status')).toContainText('Input exceeds');await expect(page.locator('#download')).toBeDisabled();
});

test('local run-request download retains graph and all controls without engine access',async({page})=>{
 await page.route('**/*.wasm',route=>route.abort());await page.goto('./');await page.locator('#input').fill(JSON.stringify(graph));await page.locator('#target').fill('review');await page.locator('#kind').selectOption('presentation');await page.locator('details').filter({has:page.locator('#pointer')}).locator('summary').click();await page.locator('#pointer').fill(pointer);
 const [file]=await Promise.all([page.waitForEvent('download'),page.locator('#request').click()]);const request=JSON.parse(fs.readFileSync(await file.path(),'utf8'));expect(request).toEqual({graph_text:JSON.stringify(graph),target:'review',kind:'presentation',pointer_text:pointer});
});
