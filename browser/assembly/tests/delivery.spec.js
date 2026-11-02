import {test,expect} from '@playwright/test';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';

test('every packaged file matches inventory and exact source provenance',async({page,request})=>{
 test.setTimeout(180000);
 await page.goto('./');
 await expect(page.getByRole('link',{name:'Native Rust source',exact:true})).toHaveAttribute('href','https://github.com/giodl73-repo/REEL/tree/main/crates/reel-assembly/src');
 const inventory=await (await request.get('./inventory.json')).json();
 const provenance=await (await request.get('./provenance.json')).json();
 const expected=process.env.REEL_EXPECTED_COMMIT || execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
 expect(provenance.commit).toBe(expected);expect(inventory.commit).toBe(expected);
 const landing=await request.get('../index.html');expect(landing.ok()).toBe(true);expect(createHash('sha256').update(await landing.body()).digest('hex')).toBe(provenance.landing_index_sha256);
 for(const entry of inventory.files){
  const response=await request.get('./'+entry.path);expect(response.ok(),entry.path).toBe(true);
  const data=await response.body();expect(data.length,entry.path).toBe(entry.bytes);expect(createHash('sha256').update(data).digest('hex'),entry.path).toBe(entry.sha256);
 }
 const wasm=await request.get('./pkg/reel_assembly_workbench_bg.wasm');expect(wasm.headers()['content-type']).toContain('application/wasm');
});

test('source, build and legal routes survive WASM startup failure',async({page,request})=>{
 await page.route('**/*.wasm',route=>route.abort());await page.goto('./');await expect(page.locator('#status')).toContainText(/failed|exceeded/,{timeout:15000});
 for(const name of ['Complete Rust source ZIP','Compiled WASM','Build instructions','Build provenance','Licenses and notices']){
  const link=page.getByRole('link',{name,exact:true});await expect(link).toBeVisible();
  const url=await link.getAttribute('href');expect((await request.get(url)).ok(),name).toBe(true);
 }
});
