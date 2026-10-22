'use strict';
const graph = JSON.parse(document.getElementById('data').textContent);
const byId = id => document.getElementById(id);
const entries = Object.entries(graph.nodes);
let pageLimit = 200;
const describe = kind => ({
  'inventory-member': 'Inventory observation; does not establish a build dependency.',
  'requires-version': 'Exact dependency recorded in the native version registry.',
  'requires-asset': 'Required selected input returned by the native scene resolver; media presence is size-only.',
  'selected-source-text': 'Native selected source-text metadata, freshly hash checked when opted in; original manuscript and audio remain unverified.',
  'authoring-document': 'Explicit metadata input to native scene resolution; its content hash is recorded.',
  'source-authority': 'Declared source authority and scope; does not establish authorship, consent or approval.',
  'source-document': 'Exact source revision bytes; content and declared scope have not been verified.',
  'authority-evidence': 'Opaque authority evidence; not interpreted as a permission or approval.',
  'delivers-version': 'Exact historical subject version; not a build dependency or current selection.',
  'delivers-asset': 'Declared output retained by the subject version; no remote readback or release permission.',
  'delivery-evidence': 'Opaque delivery receipt evidence; no destination was accessed.',
  'retains-asset': 'Bytes retained by an immutable version; not creative approval.',
  'describes-version': 'Context or scope; not a build dependency.',
  'episode-context': 'Episode backlink; not a required dependency cycle.'
}[kind] || 'Typed reference; inspect its field and findings before deciding its meaning.');
byId('summary').textContent = `${graph.project_id} (registry snapshot ${graph.registry_generation}): ${entries.length} nodes, ${graph.edges.length} relationships, ${graph.findings.length} findings`;
byId('scope').textContent = graph.scope;
for (const kind of [...new Set(graph.edges.map(e => e.kind))].sort()) {
  const o = document.createElement('option'); o.value = kind; o.textContent = kind; byId('kind').append(o);
}
function link(container, id, label) {
  const b = document.createElement('button'); b.type = 'button'; b.textContent = label;
  b.addEventListener('click', () => select(id)); container.append(b);
}
function select(id) {
  const n = graph.nodes[id]; if (!n) return;
  byId('selection').textContent = id;
  byId('meaning').textContent = `${n.kind} · ${n.status}. This saved graph does not authorize removal or certify build readiness.`;
  byId('details').textContent = JSON.stringify(n, null, 2);
  for (const direction of ['incoming', 'outgoing']) {
    const container = byId(direction); container.replaceChildren();
    const edges = graph.edges.filter(e => direction === 'incoming' ? e.to === id : e.from === id);
    for (const e of edges) {
      const target = direction === 'incoming' ? e.from : e.to;
      const row = document.createElement('p');
      link(row, target, `${e.kind}: ${target}`);
      const detail = document.createElement('span'); detail.textContent = ` ${e.field} — ${describe(e.kind)}`; row.append(detail); container.append(row);
    }
    if (!edges.length) container.textContent = 'None recorded in this scoped inspection.';
  }
  byId('findings').textContent = JSON.stringify(graph.findings.filter(f => f.node === id), null, 2);
}
function render() {
  const q = byId('search').value.toLowerCase(), kind = byId('kind').value;
  const allowed = new Set(graph.edges.filter(e => !kind || e.kind === kind).flatMap(e => [e.from, e.to]));
  const filtered = entries.filter(([id, n]) => (!kind || allowed.has(id)) && `${id} ${n.schema || ''} ${n.status}`.toLowerCase().includes(q));
  byId('nodes').replaceChildren();
  for (const [id, n] of filtered.slice(0, pageLimit)) link(byId('nodes'), id, `${id} · ${n.status}`);
  byId('count').textContent = `${Math.min(pageLimit, filtered.length)} of ${filtered.length} matching nodes. Findings include unsupported or uninspected fields.`;
  byId('more').hidden = filtered.length <= pageLimit;
}
byId('search').addEventListener('input', () => { pageLimit = 200; render(); });
byId('kind').addEventListener('change', () => { pageLimit = 200; render(); });
byId('more').addEventListener('click', () => { pageLimit += 200; render(); });
render();
