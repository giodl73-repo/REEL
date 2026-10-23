const test=require('node:test');const assert=require('node:assert/strict');const ui=require('../src/cel_sorter.js');
const rows=[{semantic_id:'a',picture_binding:'one',frames:41},{semantic_id:'b',picture_binding:'two',frames:59}];
test('frame starts follow reordered holds',()=>assert.deepEqual(ui.starts([...rows].reverse()),[0,59]));
test('fit is exact, positive and preserves stable identities',()=>{const fit=ui.fit(rows,123);assert.equal(fit.reduce((s,r)=>s+r.frames,0),123);assert.equal(fit[0].semantic_id,'a');assert(fit.every(r=>r.frames>0));assert.equal(rows[0].frames,41);});
test('reject duplicates, missing coverage and foreign images',()=>{assert(ui.valid(rows,rows,100));assert(!ui.valid([rows[0],rows[0]],rows,82));assert(!ui.valid(rows,rows,99));assert(!ui.valid([{...rows[0],picture_binding:'foreign',frames:100}],rows,100));});
test('removal can fit remaining image to full recording',()=>{const fit=ui.fit([rows[0]],100);assert(ui.valid(fit,rows,100));});
