import {test, expect} from '@playwright/test';
import fs from 'node:fs';
const source=fs.readFileSync(new URL('../fixtures/graph.json',import.meta.url),'utf8').replace(/\r\n?/g,'\n');
const fixture=JSON.parse(source);
async function ready(page){await page.goto('./');await expect(page.locator('#status')).toContainText('Ready.');}
async function inspect(page){await page.locator('#inspect').click();await expect(page.locator('#status')).toContainText(/Graph inspected|Input was rejected/);}
async function download(page,id){const [file]=await Promise.all([page.waitForEvent('download'),page.locator(id).click()]);return fs.readFileSync(await file.path());}

test('raw BOM and CRLF remain separate from explicit LF copy and edited text',async({page})=>{
 await ready(page);
 const raw=Buffer.from('\uFEFF'+source.replace(/\n/g,'\r\n'),'utf8');
 await page.locator('#file').setInputFiles({name:'own-graph.json',mimeType:'application/json',buffer:raw});
 await expect(page.locator('#file-status')).toContainText('original bytes retained');
 expect(await download(page,'#original')).toEqual(raw);expect(await download(page,'#current')).toEqual(raw);
 await inspect(page);expect(JSON.parse(await page.locator('#report').textContent()).ok).toBe(false);
 await page.locator('#lf-copy').click();await expect(page.locator('#download')).toBeDisabled();
 await inspect(page);const report=JSON.parse(await page.locator('#report').textContent());expect(report.ok).toBe(true);expect(report.input_bytes).toBe(Buffer.byteLength(source));
 expect(await download(page,'#current')).toEqual(Buffer.from(source));expect(await download(page,'#original')).toEqual(raw);
 await page.locator('#input').fill('{}');await expect(page.locator('#download')).toBeDisabled();expect(await download(page,'#original')).toEqual(raw);
});

test('invalid UTF-8 and oversized files retain previous document and original',async({page})=>{
 await ready(page);await page.locator('#file').setInputFiles({name:'first.json',mimeType:'application/json',buffer:Buffer.from(source)});
 await expect(page.locator('#file-status')).toContainText('Opened first.json');
 for(const [name,buffer] of [['invalid.json',Buffer.from([0xff])],['large.json',Buffer.alloc(131073,32)]]){
  await page.locator('#file').setInputFiles({name,mimeType:'application/json',buffer});
  await expect(page.locator('#file-status')).toContainText(/retained/);
  expect(await download(page,'#current')).toEqual(Buffer.from(source));expect(await download(page,'#original')).toEqual(Buffer.from(source));
 }
});

test('own asset declarations and labels stay inert',async({page})=>{
 await ready(page);const v=structuredClone(fixture);v.slots[0].revisions[0].asset.logical_id='<svg/onload=alert(1)>';
 await page.locator('#input').fill(JSON.stringify(v));await inspect(page);
 await expect(page.locator('#summary')).toContainText('<svg/onload=alert(1)>');expect(await page.locator('#summary a,#summary img,#summary svg').count()).toBe(0);
 const report=JSON.parse((await download(page,'#download')).toString('utf8'));expect(report.ok).toBe(true);expect(report.closure.selected_assets.length).toBe(2);
});

test('oversized paste and drop reject whole input without truncation',async({page})=>{
 await ready(page);await page.locator('#input').fill(source);
 for(const type of ['paste','drop']){
  await page.locator('#input').evaluate((input,type)=>{input.setSelectionRange(input.value.length,input.value.length);const data=new DataTransfer();data.setData('text/plain','x'.repeat(131073));const event=type==='paste'?new ClipboardEvent('paste',{clipboardData:data,bubbles:true,cancelable:true}):new DragEvent('drop',{dataTransfer:data,bubbles:true,cancelable:true});input.dispatchEvent(event);},type);
  await expect(page.locator('#file-status')).toContainText('Entire operation rejected');expect(await download(page,'#current')).toEqual(Buffer.from(source));
 }
});

test('slow file completion cannot overwrite newer edits or file selection',async({page})=>{
 await page.addInitScript(()=>{const original=Blob.prototype.arrayBuffer;Blob.prototype.arrayBuffer=function(){const promise=original.call(this);return this.name==='slow.json'?new Promise(resolve=>setTimeout(()=>resolve(promise),800)):promise;};});
 await ready(page);await page.locator('#file').setInputFiles({name:'slow.json',mimeType:'application/json',buffer:Buffer.from(source)});
 await page.locator('#input').fill('{"newer":"edit"}');await page.waitForTimeout(1000);await expect(page.locator('#input')).toHaveValue('{"newer":"edit"}');await expect(page.locator('#original')).toBeDisabled();await expect(page.locator('#file-status')).toContainText('Pending read canceled');
 await page.locator('#file').setInputFiles({name:'slow.json',mimeType:'application/json',buffer:Buffer.from(source)});
 await page.locator('#file').setInputFiles({name:'new.json',mimeType:'application/json',buffer:Buffer.from('{}')});await expect(page.locator('#file-status')).toContainText('Opened new.json');await page.waitForTimeout(1000);expect(await download(page,'#original')).toEqual(Buffer.from('{}'));await expect(page.locator('#input')).toHaveValue('{}');
});

test('file read deadline is bounded and keeps existing input',async({page})=>{
 await page.addInitScript(()=>{Blob.prototype.arrayBuffer=function(){return new Promise(()=>{});};});
 await ready(page);await page.locator('#input').fill(source);await page.locator('#file').setInputFiles({name:'stuck.json',mimeType:'application/json',buffer:Buffer.from('{}')});
 await expect(page.locator('#file-status')).toContainText('File read exceeded 5 seconds.',{timeout:7000});await expect(page.locator('#input')).toHaveValue(source);
});

test('worker startup deadline retains input and independent downloads',async({page})=>{
 await page.route('**/worker.js',route=>route.fulfill({contentType:'text/javascript',body:'onmessage=()=>{};'}));
 await page.goto('./');await page.locator('#input').fill(source);
 await expect(page.locator('#status')).toContainText('Engine startup exceeded 12 seconds.',{timeout:15000});await expect(page.locator('#inspect')).toBeDisabled();expect(await download(page,'#current')).toEqual(Buffer.from(source));await expect(page.getByRole('link',{name:'Native Rust source'})).toBeVisible();
 await page.unroute('**/worker.js');await page.locator('#restart').click();await expect(page.locator('#status')).toContainText('Ready.');await inspect(page);
});

test('compute deadline clears exports and retry retains inputs',async({page})=>{
 await page.route('**/worker.js',route=>route.fulfill({contentType:'text/javascript',body:"postMessage({type:'ready'});onmessage=()=>{};"}));
 await ready(page);await page.locator('#input').fill(source);await page.locator('#inspect').click();
 await expect(page.locator('#status')).toContainText('Inspection exceeded 10 seconds.',{timeout:13000});await expect(page.locator('#download')).toBeDisabled();await expect(page.locator('#input')).toHaveValue(source);
 await page.unroute('**/worker.js');await page.locator('#restart').click();await expect(page.locator('#status')).toContainText('Ready.');await inspect(page);
});

test('engine errors retain input and recover through restart',async({page})=>{
 await page.route('**/worker.js',route=>route.fulfill({contentType:'text/javascript',body:"throw new Error('test worker crash');"}));
 await page.goto('./');await page.locator('#input').fill(source);await expect(page.locator('#status')).toContainText('Rust engine failed.');await page.locator('#input').fill(source+' ');await expect(page.locator('#status')).toContainText('Rust engine failed.');await page.locator('#input').fill(source);await expect(page.locator('#download')).toBeDisabled();
 await page.unroute('**/worker.js');await page.locator('#restart').click();await expect(page.locator('#status')).toContainText('Ready.');await expect(page.locator('#input')).toHaveValue(source);await inspect(page);
});

test('old worker and job callbacks cannot publish over a newer revision',async({page})=>{
 await page.addInitScript(()=>{window.workers=[];window.Worker=class{constructor(){window.workers.push(this);setTimeout(()=>this.onmessage?.({data:{type:'ready'}}),0);}postMessage(data){this.request=data;}terminate(){this.terminated=true;}};});
 await ready(page);await page.locator('#input').fill(source);await page.locator('#inspect').click();await page.locator('#input').fill('{}');
 await expect(page.locator('#status')).toContainText('Ready.');
 await page.evaluate(()=>{const first=window.workers[0];first.onmessage({data:{type:'result',id:first.request.id,text:JSON.stringify({ok:false,error:'OLD REPORT'})}});first.onerror();});
 await expect(page.locator('#download')).toBeDisabled();await expect(page.locator('#report')).toHaveText('No report yet.');await expect(page.locator('#status')).toContainText('Ready.');
 await page.locator('#inspect').click();await page.evaluate(()=>{const worker=window.workers.at(-1);worker.onmessage({data:{type:'result',id:'wrong-job',text:'{}'}});});await expect(page.locator('#download')).toBeDisabled();
 await page.evaluate(()=>{const worker=window.workers.at(-1);worker.onmessage({data:{type:'result',id:worker.request.id,text:JSON.stringify({ok:false,error:'CURRENT REPORT'})}});});await expect(page.locator('#summary')).toContainText('CURRENT REPORT');
});

test('mobile keyboard flow has visible controls and no horizontal overflow',async({page})=>{
 await page.setViewportSize({width:390,height:844});await ready(page);await page.locator('#example').click();await expect(page.locator('#input')).toHaveValue(/demo-lock/);
 await page.locator('#inspect').focus();await page.keyboard.press('Enter');await expect(page.locator('#status')).toContainText('Graph inspected');await expect(page.locator('#summary')).toContainText('Selected asset declarations (2)');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
 await page.screenshot({path:'.cache/mobile.png',fullPage:true});await page.setViewportSize({width:1440,height:1100});await page.screenshot({path:'.cache/desktop.png',fullPage:true});
});
