const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../workflows/ci.yml'), 'utf8').replace(/\r/g, '');
const job = source.slice(source.indexOf('\n  rust-tests:'), source.indexOf('\n  ffmpeg-tests:'));
const matrix = [...job.matchAll(/- os: ([\w-]+)\n\s+lane: ([\w-]+)\n\s+shard: (\d+)/g)]
  .map(match => ({ os: match[1], lane: match[2], shard: Number(match[3]) }));

function applies(condition, entry) {
  if (!condition) return true;
  const values = { 'matrix.os': entry.os, 'matrix.lane': entry.lane,
    'matrix.shard': entry.shard, 'runner.os': entry.os === 'windows-latest' ? 'Windows' : 'Linux' };
  return condition.split(' && ').every(part => {
    part = part.replace(/^\((.*)\)$/, '$1');
    const prefix = part.match(/^startsWith\(matrix.lane, '([^']+)'\)$/);
    if (prefix) return entry.lane.startsWith(prefix[1]);
    const comparison = part.match(/^(matrix\.\w+|runner\.os) == (?:'([^']+)'|(\d+))$/);
    assert.ok(comparison, `Unsupported qualification condition: ${part}`);
    return values[comparison[1]] === (comparison[2] === undefined ? Number(comparison[3]) : comparison[2]);
  });
}

test('both platforms retain every ordinary shard and common media qualification', () => {
  for (const os of ['ubuntu-latest', 'windows-latest']) {
    assert.deepEqual(matrix.filter(entry => entry.os === os && entry.lane === 'core').map(entry => entry.shard), [0, 1, 2]);
    assert.equal(matrix.filter(entry => entry.os === os && entry.lane === 'media').length, 1);
  }
  assert.equal(new Set(matrix.map(entry => JSON.stringify(entry))).size, matrix.length);
  assert.match(job, /timeout-minutes: 28/);
});

test('every qualification step executes, with no accidental per-platform duplication', () => {
  const steps = job.slice(job.indexOf('    steps:\n')).split(/(?=^      - )/m);
  let checked = 0;
  for (const step of steps) {
    if (!/^(?:      - run:|        run:)/m.test(step)) continue;
    const title = (step.match(/^      - name: (.+)$/m) || [])[1] || step.split('\n')[0];
    const condition = (step.match(/^        if: (.+)$/m) || [])[1];
    const selected = matrix.filter(entry => applies(condition, entry));
    assert.ok(selected.length > 0, `Unreachable check: ${title}`);
    if (!['Run ordinary test shard', 'Build pinned Tig authoring handoff CLI'].includes(title)) {
      for (const os of ['ubuntu-latest', 'windows-latest']) {
        assert.ok(selected.filter(entry => entry.os === os).length <= 1, `Duplicated check: ${title}`);
      }
    }
    checked++;
  }
  assert.ok(checked > 50, 'Missing qualification steps');
});
