// Exercise the shipped browser script with a tiny DOM double, no dependencies or server.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const script = fs.readFileSync(path.join(__dirname, '../src/tier_review.js'), 'utf8');

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.style = {}; this.value = ''; this.checked = false; this.events = {}; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  setAttribute(name, value) { this[name] = value; }
  addEventListener(name, handler) { this.events[name] = handler; }
  click() { this.onclick?.(); }
}
function block(id, extra = {}) {
  return {id, owner: 'film@v1', class: 'previously-used', observed_tiers: ['fast', 'history'],
    proposal: 'verify-and-demote', eligible: true, protected: false, objects: 1, logical_bytes: 100,
    observed_copy_bytes: {fast: 100, history: 100}, estimated_reclaim_bytes: 100,
    disposition_reason: 'Explicit previous use; retain history', member_preview: ['a'.repeat(64)], ...extra};
}
function fixture(blocks = [block('one'), block('held', {eligible: false, protected: true})], extra = {}) {
  const nodes = new Map(), downloads = [];
  const document = {
    getElementById(id) { if (!nodes.has(id)) nodes.set(id, new Element('div')); return nodes.get(id); },
    createElement(tag) { return new Element(tag); }, createElementNS(ns, tag) { return new Element(tag); }
  };
  const review = {project_id: 'independent-project', selected_tier: 'fast', history_tier: 'history', cold_tier: 'archive',
    plan_sha256: 'plan', review_sha256: 'review', baseline: {sha256: 'baseline'}, summary: {'previously-used': {}},
    tiers: [{id: 'fast', index: 0, storage: 'active'}, {id: 'archive', index: 0, storage: 'archive'}],
    blocks, max_batch_objects: 100, max_batch_bytes: 10000, expires_unix_seconds: Date.now() / 1000 + 600, ...extra};
  document.getElementById('review-data').textContent = JSON.stringify(review);
  const context = vm.createContext({document, Date, Blob, setTimeout: () => {}, setInterval: () => {},
    URL: {createObjectURL(blob) { downloads.push(blob); return 'blob:test'; }, revokeObjectURL() {}}});
  vm.runInContext(script, context);
  const get = id => document.getElementById(id);
  const input = (id, value) => { get(id).value = value; get(id).events.input(); };
  return {get, input, downloads};
}
test('starts empty, protects held rows and exports exact native decision identities', async () => {
  const {get, downloads} = fixture();
  assert.equal(get('download').disabled, true);
  assert.equal(get('rows').children[1].children[0].children[0].disabled, true);
  get('select-visible').click();
  assert.equal(get('download').disabled, false);
  get('download').click();
  assert.deepEqual(JSON.parse(await downloads[0].text()), {schema: 'reel.asset-tier-decision.v1', review_sha256: 'review', plan_sha256: 'plan', action: 'remove-redundant-hot-copy', selected_blocks: ['one']});
});
test('filters by tier, hash preview and reason, sorts largest content first', () => {
  const {get, input} = fixture([block('small'), block('large', {logical_bytes: 400, observed_tiers: ['history'], member_preview: ['b'.repeat(64)]})]);
  input('sort', 'size');
  assert.match(get('rows').children[0].children[3].children[1].textContent, /400/);
  input('tier', 'fast'); assert.equal(get('rows').children.length, 1);
  input('search', 'bbbbbbbb'); assert.equal(get('rows').children.length, 0);
  input('tier', ''); assert.equal(get('rows').children.length, 1);
  input('search', 'retain history'); assert.equal(get('rows').children.length, 2);
});
test('selection survives filters and chosen-only view can clear it', () => {
  const {get, input} = fixture();
  get('select-visible').click(); input('search', 'not-found');
  assert.match(get('selection').textContent, /^1 blocks/);
  input('search', ''); get('chosen-only').checked = true; get('chosen-only').events.input();
  assert.equal(get('rows').children.length, 1);
  get('clear').click(); assert.equal(get('rows').children.length, 0);
  assert.equal(get('download').disabled, true);
});
test('pagination chooses only the visible actionable page', () => {
  const {get} = fixture(Array.from({length: 65}, (_, i) => block('b' + i)));
  assert.equal(get('rows').children.length, 60);
  get('next').click(); assert.equal(get('rows').children.length, 5);
  get('select-visible').click(); assert.match(get('selection').textContent, /^5 blocks/);
});
test('expired and over-budget choices cannot download', () => {
  for (const extra of [{expires_unix_seconds: 1}, {max_batch_objects: 0}, {max_batch_bytes: 1}]) {
    const {get, downloads} = fixture([block('one')], extra);
    get('select-visible').click(); get('download').click();
    assert.equal(get('download').disabled, true); assert.equal(downloads.length, 0);
  }
});
test('unknown reclaim remains unknown and private labels use literal text', () => {
  const hostile = '</script><img src=https://example.invalid>';
  const {get} = fixture([block('one', {owner: hostile, estimated_reclaim_bytes: null})]);
  assert.equal(get('rows').children[0].children[1].children[0].textContent, hostile);
  get('select-visible').click();
  assert.match(get('selection').textContent, /partly unknown/);
});
