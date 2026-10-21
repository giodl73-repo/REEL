'use strict';

const review = JSON.parse(document.getElementById('review-data').textContent);
const chosen = new Set();
const $ = id => document.getElementById(id);
const labels = {
  selected: 'Selected and required inputs',
  'previously-used': 'Previously used',
  'current-working-hold': 'Current working set',
  'unregistered-cold-candidate': 'Unregistered / archive first',
  'registered-unused-or-unproven': 'Registered / disposition needed',
  'registration-removed-hold': 'Registration removed / investigate',
  'registry-control': 'Registry controls',
  'active-lease': 'Active lease',
  'tier-pin': 'Pinned tier'
};
const colors = ['#337965', '#628daa', '#d3aa5c', '#ab7770', '#819487', '#697382', '#8a7c99'];
const names = Object.keys(review.summary);
const color = key => colors[names.indexOf(key) % colors.length];
const fmt = n => {
  if (n === null || n === undefined) return 'Unknown';
  let value = n, unit = 0;
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return value.toLocaleString(undefined, {maximumFractionDigits: unit ? 1 : 0}) + ' ' + units[unit];
};
const text = (tag, value, cls) => {
  const node = document.createElement(tag);
  node.textContent = value;
  if (cls) node.className = cls;
  return node;
};
const sum = (blocks, field) => blocks.reduce((n, block) => n + block[field], 0);

$('intro').textContent = review.project_id + ' · ' + review.blocks.length.toLocaleString() + ' blocks · private offline review';
$('limits').textContent = review.limits;
$('placement').textContent = 'Selected → ' + review.selected_tier + ' · Previous use → ' + review.history_tier + ' · Unregistered → ' + review.cold_tier + ' after custody verification';
const eligible = review.blocks.filter(b => b.eligible);
for (const [label, value, note] of [
  ['Inventoried logical content', fmt(sum(review.blocks, 'logical_bytes')), 'Shared hashes counted once; not disk allocation'],
  ['Historical removal candidates', fmt(sum(eligible, 'logical_bytes')), sum(eligible, 'objects').toLocaleString() + ' objects; verify retained bytes first'],
  ['Protected / held content', fmt(sum(review.blocks.filter(b => !b.eligible), 'logical_bytes')), 'Inspect each block for its reason']
]) {
  const card = text('div', '', 'card');
  card.append(text('div', label, 'muted'), text('div', value, 'metric'), text('small', note));
  $('metrics').append(card);
}
for (const key of names) {
  const item = text('span', ''), dot = text('i', '', 'dot');
  dot.style.background = color(key);
  item.append(dot, text('span', labels[key] || key));
  $('legend').append(item);
  const option = text('option', labels[key] || key);
  option.value = key;
  $('class').append(option);
}
for (const tier of review.tiers) {
  const option = text('option', tier.id);
  option.value = tier.id;
  $('tier').append(option);
  const box = text('div', '', 'tier');
  box.append(text('strong', tier.id));
  const observed = (review.tier_observations || []).find(t => t.tier === tier.index && tier.storage === 'active');
  if (observed) box.append(text('div', observed.root + ' · free at census ' + fmt(observed.free_bytes) + ' of ' + fmt(observed.total_bytes), 'muted'));
  if (tier.storage === 'archive') {
    box.append(text('p', 'Not inventoried here. Archive custody requires an explicit operation.', 'muted'));
    $('tiers').append(box);
    continue;
  }
  const values = names.map(key => review.blocks.filter(b => b.class === key).reduce((n, b) => n + (b.observed_copy_bytes[tier.id] || 0), 0));
  const total = values.reduce((a, b) => a + b, 0);
  box.append(text('span', ' · ' + fmt(total) + ' observed logical copies', 'muted'));
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 1000 60');
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', tier.id + ' observed copy bytes by classification');
  let x = 0;
  values.forEach((n, i) => {
    if (!n || !total) return;
    const rect = document.createElementNS(svg.namespaceURI, 'rect');
    for (const [key, value] of Object.entries({x, y: 8, width: n / total * 1000, height: 32, fill: color(names[i])})) rect.setAttribute(key, String(value));
    const title = document.createElementNS(svg.namespaceURI, 'title');
    title.textContent = (labels[names[i]] || names[i]) + ': ' + fmt(n);
    rect.append(title);
    svg.append(rect);
    x += n / total * 1000;
  });
  box.append(svg);
  $('tiers').append(box);
}
for (const [label, value] of [
  ['Plan', review.plan_sha256], ['Review', review.review_sha256], ['Inventory baseline', review.baseline.sha256],
  ['Inventory census completed', review.census_completed_unix_seconds ? new Date(review.census_completed_unix_seconds * 1000).toLocaleString() : 'Not recorded'],
  ['Baseline generation', review.baseline_generation], ['Reconciled generation', review.live_generation],
  ['New registrations since census', review.delta?.new_registered_objects ?? 'Unknown'],
  ['Scope', review.inventory_scope], ['Created', new Date(review.created_unix_seconds * 1000).toLocaleString()],
  ['Expires', new Date(review.expires_unix_seconds * 1000).toLocaleString()]
]) $('identity').append(text('p', label + ': ' + value));

let page = 0;
const pageSize=60;
function filtered() {
  const query = $('search').value.trim().toLowerCase(), cls = $('class').value, tier = $('tier').value;
  const rows = review.blocks.filter(b => (!cls || b.class === cls) && (!tier || b.observed_tiers.includes(tier)) &&
    (!$('eligible').checked || b.eligible) && (!$('chosen-only').checked || chosen.has(b.id)) &&
    (!query || [b.owner, b.class, b.disposition_reason, ...b.observed_tiers, b.id, ...b.member_preview].join(' ').toLowerCase().includes(query)));
  if ($('sort').value === 'size') rows.sort((a, b) => b.logical_bytes - a.logical_bytes || a.id.localeCompare(b.id));
  if ($('sort').value === 'reclaim') rows.sort((a, b) => (b.estimated_reclaim_bytes ?? -1) - (a.estimated_reclaim_bytes ?? -1) || a.id.localeCompare(b.id));
  return rows;
}
function updateSelection() {
  const blocks = review.blocks.filter(b => chosen.has(b.id));
  const count = sum(blocks, 'objects'), bytes = sum(blocks, 'logical_bytes');
  const unknown = blocks.some(b => b.estimated_reclaim_bytes === null);
  const estimate = blocks.reduce((n, b) => n + (b.estimated_reclaim_bytes || 0), 0);
  const expired = Date.now() / 1000 >= review.expires_unix_seconds;
  const over = count > review.max_batch_objects || bytes > review.max_batch_bytes;
  $('selection').textContent = blocks.length ? blocks.length + ' blocks · ' + count.toLocaleString() + ' objects · ' + fmt(bytes) + ' logical · estimated reclaim ' + (unknown ? 'partly unknown' : fmt(estimate)) : 'No blocks chosen';
  $('budget').textContent = 'Batch limit: ' + review.max_batch_objects.toLocaleString() + ' objects / ' + fmt(review.max_batch_bytes);
  $('status').textContent = expired ? 'This snapshot expired. Generate a fresh review.' : over ? 'Choose fewer blocks to stay within the batch limit.' : '';
  $('download').disabled = !blocks.length || expired || over;
}
function render() {
  const all = filtered();
  page = Math.min(page, Math.max(0, Math.ceil(all.length / pageSize) - 1));
  $('rows').replaceChildren();
  for (const block of all.slice(page * pageSize, (page + 1) * pageSize)) {
    const row = document.createElement('tr'), pick = document.createElement('td'), cb = document.createElement('input');
    cb.type = 'checkbox'; cb.disabled = !block.eligible; cb.checked = chosen.has(block.id);
    cb.setAttribute('aria-label', 'Choose ' + block.owner + ' ' + block.id.slice(0, 8));
    cb.addEventListener('change', () => { if (cb.checked) chosen.add(block.id); else chosen.delete(block.id); render(); });
    pick.append(cb);
    const title = document.createElement('td');
    title.append(text('strong', block.owner), text('div', labels[block.class] || block.class, 'muted'));
    const details = document.createElement('details');
    details.append(text('summary', 'Inspect block'));
    details.append(text('p', block.disposition_reason));
    details.append(text('small', 'Block: ' + block.id));
    details.append(text('p', 'Content hashes (' + block.member_preview.length + ' of ' + block.objects + ' shown)'));
    for (const hash of block.member_preview) details.append(text('div', hash, 'hash'));
    details.append(text('p', 'One version reference is shown. Shared content may also serve other versions.', 'muted'));
    title.append(details);
    const location = document.createElement('td');
    location.append(text('div', block.observed_tiers.join(' + ') || 'No observed copy'), text('small', 'Integrity not yet proven'));
    const size = document.createElement('td');
    size.append(text('div', block.objects.toLocaleString()), text('small', fmt(block.logical_bytes)));
    const state = document.createElement('td');
    state.append(text('span', block.eligible ? 'Can request verification' : block.protected ? 'Protected' : 'Held / separate action', block.eligible ? 'badge ready' : 'badge'), text('div', block.disposition_reason, 'muted'));
    row.append(pick, title, location, size, text('td', fmt(block.estimated_reclaim_bytes)), state);
    $('rows').append(row);
  }
  $('empty').hidden = all.length > 0;
  $('counts').textContent = all.length.toLocaleString() + ' matching blocks · ' + fmt(sum(all, 'logical_bytes')) + ' logical';
  $('page').textContent = (page + 1) + ' / ' + Math.max(1, Math.ceil(all.length / pageSize));
  $('prev').disabled = page === 0;
  $('next').disabled = (page + 1) * pageSize >= all.length;
  updateSelection();
}
for (const id of ['search', 'class', 'tier', 'sort', 'eligible', 'chosen-only']) $(id).addEventListener('input', () => { page = 0; render(); });
$('prev').onclick = () => { page--; render(); };
$('next').onclick = () => { page++; render(); };
$('clear').onclick = () => { chosen.clear(); render(); };
$('select-visible').onclick = () => { for (const b of filtered().slice(page * pageSize, (page + 1) * pageSize)) if (b.eligible) chosen.add(b.id); render(); };
$('download').onclick = () => {
  updateSelection();
  if ($('download').disabled) return;
  const decision = {schema: 'reel.asset-tier-decision.v1', review_sha256: review.review_sha256, plan_sha256: review.plan_sha256, action: 'remove-redundant-hot-copy', selected_blocks: [...chosen].sort()};
  const url = URL.createObjectURL(new Blob([JSON.stringify(decision, null, 2) + '\n'], {type: 'application/json'}));
  const a = document.createElement('a');
  a.href = url; a.download = 'cache-block-choice-' + review.review_sha256.slice(0, 12) + '.json'; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
};
function freshness() {
  const expired = Date.now() / 1000 >= review.expires_unix_seconds;
  $('freshness').textContent = (expired ? 'Expired review. ' : 'Snapshot review. ') + review.copy_proof + ' New unregistered files require an explicit inventory refresh; changed selections require a fresh review.';
  updateSelection();
}
render(); freshness(); setInterval(freshness, 15000);
