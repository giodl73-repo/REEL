const $ = id => document.getElementById(id);
const input = $('input'), MAX_BYTES = 131072;
let worker, generation = 0, revision = 0, jobTimer, ready = false;
let report = null, pending = null, currentText = '', original = null;
let readToken = 0, readController = null, reading = false, engineMessage = 'Starting Rust engine...';
const bytes = text => new TextEncoder().encode(text).length;
const status = message => { $('status').textContent = message; };
function updateControls() {
  $('bytes').textContent = `${bytes(currentText).toLocaleString()} UTF-8 bytes`;
  $('inspect').disabled = !ready || pending !== null;
  $('lf-copy').disabled = !currentText;
}
function clearReport() {
  report = null; $('download').disabled = true; $('report').textContent = 'No report yet.';
  $('result-state').textContent = 'No report';
  $('summary').replaceChildren(element('p', 'Inspect the current input to see its declarations.', 'empty'));
}
function element(tag, text, className) {
  const node = document.createElement(tag); node.textContent = text;
  if (className) node.className = className;
  return node;
}
function render(result) {
  const container=$('summary');container.replaceChildren();
  $('result-state').textContent=result.ok?'Calculated':'Rejected';
  container.append(element('p',result.scope,'boundary'));
  if ('graph_valid' in result) container.append(element('p',`Native structural validation: ${result.graph_valid}. Whole-graph acyclicity and media availability are separate.`));
  if ('declared_lock_matches_computed' in result && result.computed_graph_lock) {
    container.append(element('h3','Lock observations'));
    container.append(element('p',`Declared lock matches native computed lock: ${result.declared_lock_matches_computed}.`));
    container.append(element('p',`Computed graph SHA-256: ${result.computed_graph_lock.sha256}`,'command'));
    container.append(element('p',`Pointer matches supplied graph lock: ${result.pointer_matches_supplied_graph_lock ?? 'No pointer supplied'}. This is not cache-byte verification.`));
  }
  if (!result.ok) {container.append(element('h3',result.stage));container.append(element('p',result.error,'error'));return;}
  const closure=result.closure;
  container.append(element('h3',`Native closure: ${closure.target}`));
  container.append(element('p',`Digest: ${closure.digest_sha256}`,'command'));
  container.append(element('h3',`Included nodes (${closure.node_ids.length})`));
  for(const id of closure.node_ids) container.append(element('p',id,'command'));
  container.append(element('h3',`Selected asset declarations (${closure.selected_assets.length})`));
  if(!closure.selected_assets.length) container.append(element('p','No selected asset declarations. This does not mean a production is complete.'));
  for(const asset of closure.selected_assets) {const row=element('div','','command');row.append(element('strong',asset.logical_id),element('p',asset.cache_uri),element('p',asset.sha256));container.append(row);}
  container.append(element('h3','All graph slot dispositions'));
  for(const slot of result.graph.slots) container.append(element('p',`${slot.slot_id} / ${slot.lane}: ${slot.disposition}; selected revision: ${slot.selected_revision_id ?? '(none)'}`,'command'));
  container.append(element('p','Native closure omits unselected slots and does not fetch selected assets. Slot dispositions above cover the supplied graph, including nodes outside this closure.','boundary'));
  container.append(element('h3',`Included semantic events (${closure.semantic_events.length})`));
  for(const event of closure.semantic_events) container.append(element('p',`${event.event_id} / ${event.language}: ${event.phrase_start_seconds}–${event.phrase_end_seconds} seconds; narration ${event.narration.logical_id}; picture ${event.picture.logical_id}`,'command'));
  if(result.presentation) container.append(element('p',`Presentation ${result.presentation.target_id}: contract ${result.presentation.contract.logical_id}, digest ${result.presentation.digest_sha256}`,'command'));
  if(result.selected) container.append(element('p',`Pointer-bound digest: ${result.selected.digest_sha256}`,'command'));
}
function invalidateRead() { readToken++; readController?.abort(); readController = null; if (reading) $('file-status').textContent = 'Pending read canceled; current input and original retained.'; reading = false; }
function changed(text, message) {
  revision++; currentText = text; clearReport();
  // Superseded work cannot publish a report or timeout into the new revision.
  if (pending) start();
  updateControls(); if (message) status(ready ? message : `${message} ${engineMessage}`);
}
function fail(message) {
  clearTimeout(jobTimer); generation++; worker?.terminate(); worker = null;
  ready = false; pending = null; clearReport(); updateControls();
  engineMessage = `${message} Restart engine to retry. Inputs and opened original are retained.`; status(engineMessage);
}
function start() {
  worker?.terminate(); clearTimeout(jobTimer); generation++;
  ready = false; pending = null; clearReport(); updateControls(); engineMessage = 'Starting Rust engine...'; status(engineMessage);
  const own = generation, startTime = performance.now();
  try { worker = new Worker('./worker.js', {type: 'module'}); }
  catch { fail('Rust worker could not start.'); return; }
  const active = worker;
  jobTimer = setTimeout(() => { if (own === generation) fail('Engine startup exceeded 12 seconds.'); }, 12000);
  active.onerror = active.onmessageerror = () => { if (own === generation && active === worker) fail('Rust engine failed.'); };
  active.onmessage = ({data}) => {
    if (own !== generation || active !== worker) return;
    if (data.type === 'ready' && !ready) {
      clearTimeout(jobTimer); ready = true; updateControls();
      $('status').dataset.startupMs = String(performance.now() - startTime);
      if (Number.isFinite(data.initialization_ms)) $('status').dataset.workerInitializationMs = String(data.initialization_ms);
      status('Ready. Inspect your graph or load the example.'); return;
    }
    if (!pending || data.id !== pending.id) return;
    clearTimeout(jobTimer); const request = pending; pending = null; updateControls();
    if (request.revision !== revision) return;
    if (data.type === 'error') { fail(data.message); return; }
    try {
      const result = JSON.parse(data.text); report = data.text;
      $('status').dataset.queryRoundtripMs = String(performance.now() - request.started);
      $('status').dataset.inputBytes = String(request.inputBytes);
      if (Number.isFinite(data.computation_ms)) $('status').dataset.workerComputationMs = String(data.computation_ms);
      $('report').textContent = report; $('download').disabled = false; render(result);
      status(result.ok ? 'Graph inspected. No media was opened.' : 'Input was rejected; see the report.');
    } catch { fail('Rust engine returned an unreadable report.'); }
  };
}
input.addEventListener('input', () => { invalidateRead(); changed(input.value, 'Input changed; previous report cleared.'); });
function insert(text, event) {
  const next = input.value.slice(0, input.selectionStart) + text + input.value.slice(input.selectionEnd);
  if (bytes(next) > MAX_BYTES) {
    event.preventDefault(); $('file-status').textContent = 'Paste/drop exceeds 128 KiB UTF-8. Entire operation rejected; previous input retained.'; return;
  }
  // Drop is inserted explicitly at the current selection; paste uses browser editing.
  if (event.type === 'drop') {
    event.preventDefault(); input.setRangeText(text, input.selectionStart, input.selectionEnd, 'end');
    input.dispatchEvent(new Event('input', {bubbles: true}));
  }
}
input.addEventListener('paste', event => insert(event.clipboardData.getData('text/plain'), event));
input.addEventListener('dragover', event => event.preventDefault());
input.addEventListener('drop', event => {
  if (event.dataTransfer.files.length) { event.preventDefault(); openFile(event.dataTransfer.files[0]); }
  else insert(event.dataTransfer.getData('text/plain'), event);
});
$('inspect').onclick = () => {
  if (!ready || pending) return; clearReport();
  if (bytes($('pointer').value)>8192 || bytes($('target').value)>256) { status('Input was rejected: pointer/target limit exceeded; nothing was truncated.'); return; }
  if (bytes(currentText) > MAX_BYTES) { status('Input exceeds 128 KiB UTF-8; nothing was truncated.'); return; }
  const id = crypto.randomUUID(); pending = {id, revision, started: performance.now(), inputBytes: bytes(currentText)}; updateControls(); status('Inspecting with Rust...');
  const own = generation;
  jobTimer = setTimeout(() => { if (own === generation) fail('Inspection exceeded 10 seconds.'); }, 10000);
  worker.postMessage({id, text: currentText, target: $('target').value, kind: $('kind').value, pointer: $('pointer').value});
};
for(const id of ['target','kind','pointer']) $(id).addEventListener('input', () => { invalidateRead(); changed(currentText, 'Request changed; previous report cleared.'); });
$('restart').onclick = start;
$('lf-copy').onclick = () => {
  invalidateRead(); const text = currentText.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n');
  input.value = text; changed(text, 'Explicit LF copy created; opened original bytes remain downloadable.');
};
async function openFile(file) {
  invalidateRead(); const token = readToken, ownRevision = revision;
  if (file.size > MAX_BYTES) { $('file-status').textContent = 'File exceeds 128 KiB UTF-8; previous input and original retained.'; return; }
  reading = true; $('file-status').textContent = `Reading ${file.name}...`;
  let timer;
  try {
    const buffer = await Promise.race([file.arrayBuffer(), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('File read exceeded 5 seconds.')), 5000); })]);
    if (token !== readToken || ownRevision !== revision) return;
    const raw = new Uint8Array(buffer);
    if (raw.length > MAX_BYTES) throw new Error('File exceeds 128 KiB UTF-8.');
    const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(raw);
    original = {raw, name: file.name}; $('original').disabled = false;
    input.value = text; changed(text, 'File loaded. Inspect it or edit your input.');
    $('file-status').textContent = `Opened ${file.name}: ${raw.length.toLocaleString()} original bytes retained.`;
  } catch (error) {
    if (token === readToken && ownRevision === revision) $('file-status').textContent = `${error.message} Previous input and original retained.`;
  } finally { clearTimeout(timer); if (token === readToken) reading = false; }
}
$('open-file').onclick = () => $('file').click();
$('file').onchange = event => { const file = event.target.files[0]; event.target.value = ''; if (file) openFile(file); };
$('example').onclick = async () => {
  invalidateRead(); const token = readToken, ownRevision = revision;
  const controller = new AbortController(); readController = controller; reading = true; $('file-status').textContent = 'Reading authored example...';
  const timer = setTimeout(() => controller.abort(), 5000);
  try {
    const response = await fetch('./graph.json', {signal: controller.signal});
    if (!response.ok) throw new Error('Example unavailable.');
    const text = await response.text();
    if (token !== readToken || ownRevision !== revision) return;
    if (bytes(text) > MAX_BYTES) throw new Error('Example exceeds adapter limit.');
    input.value = text; changed(text, 'Authored example loaded. Inspect it or edit your own input.');
    $('file-status').textContent = 'Authored example loaded; any opened original remains separately downloadable.';
  } catch (error) {
    if (token === readToken && ownRevision === revision) $('file-status').textContent = `Example could not load (${error.message}). Your input is retained.`;
  } finally { clearTimeout(timer); if (readController === controller) readController = null; if (token === readToken) reading = false; }
};
function download(data, name, type) {
  const url = URL.createObjectURL(new Blob([data], {type}));
  const a = document.createElement('a'); a.href = url; a.download = name; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
$('original').onclick = () => { if (original) download(original.raw, original.name, 'application/octet-stream'); };
$('current').onclick = () => download(currentText, 'reel-current-graph.json', 'text/plain;charset=utf-8');
$('request').onclick = () => download(JSON.stringify({graph_text:currentText,target:$('target').value,kind:$('kind').value,pointer_text:$('pointer').value},null,2),'reel-local-assembly-request.json','application/json');
$('download').onclick = () => { if (report) download(report, 'reel-local-dependency-report.json', 'application/json'); };
updateControls(); start();
