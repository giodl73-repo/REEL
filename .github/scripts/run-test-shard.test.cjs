const test = require('node:test');
const assert = require('node:assert/strict');
const { SHARD_COUNT, planShards, testArguments } = require('./run-test-shard.cjs');

test('every integration target is assigned exactly once, including future targets', () => {
  const targets = ['episode_conform', 'tig_inputs', 'programme_parts', 'new_future_test', 'another_test'];
  const shards = planShards(targets);
  const all = shards.flatMap(shard => shard.targets);
  assert.equal(shards.length, SHARD_COUNT);
  assert.deepEqual([...all].sort(), [...targets].sort());
  assert.equal(new Set(all).size, targets.length);
  assert.deepEqual(shards, planShards([...targets].reverse()));
});

test('the heaviest existing tests do not share a shard', () => {
  const shards = planShards(['episode_conform', 'tig_inputs', 'episode_conform_ordered_units_programme_regressions']);
  assert.ok(shards.every(shard => shard.targets.length === 1));
});

test('only shard zero includes non-integration targets; all keep failure propagation and feature scope', () => {
  for (const index of [0, 1, 2]) {
    const args = testArguments({ index, targets: ['some_test'] });
    assert.ok(args.includes('--all-features'));
    assert.ok(args.includes('--no-fail-fast'));
    assert.ok(args.includes('--locked'));
    assert.equal(args.includes('--lib'), index === 0);
    assert.equal(args.includes('--bins'), index === 0);
    assert.equal(args.includes('--examples'), index === 0);
    assert.equal(args.includes('--benches'), index === 0);
    assert.deepEqual(args.slice(-2), ['--test', 'some_test']);
  }
  assert.throws(() => testArguments({ index: 1, targets: [] }));
  assert.throws(() => testArguments({ index: 3, targets: [] }));
  assert.throws(() => planShards(['same', 'same']));
});
