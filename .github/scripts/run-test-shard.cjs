const { execFileSync, spawnSync } = require('node:child_process');

const SHARD_COUNT = 3;
// Seconds from the completed public Windows run 44786488240. New targets get a
// conservative default weight and are always included; this is not a test list.
const WEIGHTS = {
  episode_conform: 247,
  tig_inputs: 228,
  episode_conform_ordered_units_programme_regressions: 210,
  episode_conform_ordered_units: 196,
  programme_prefix: 144,
  programme_parts: 116,
  scene_delivery: 74,
  presentation_adopt: 55,
  tig_pcm_recovery: 40,
  scene_build_cli: 23,
  imported_scene_proof: 22,
  presentation_adopt_programme_regressions: 21,
  tig_episode: 20,
};

function planShards(targets) {
  if (new Set(targets).size !== targets.length || targets.some(name => !/^[\w-]+$/.test(name))) {
    throw new Error('Invalid or duplicate integration test target');
  }
  const shards = Array.from({ length: SHARD_COUNT }, (_, index) => ({
    index, estimatedSeconds: index === 0 ? 108 : 0, targets: [],
  }));
  const ordered = [...targets].sort((a, b) =>
    (WEIGHTS[b] || 10) - (WEIGHTS[a] || 10) || (a < b ? -1 : a > b ? 1 : 0));
  for (const target of ordered) {
    const shard = [...shards].sort((a, b) => a.estimatedSeconds - b.estimatedSeconds || a.index - b.index)[0];
    shard.targets.push(target);
    shard.estimatedSeconds += WEIGHTS[target] || 10;
  }
  return shards;
}

function testArguments(shard) {
  if (!Number.isInteger(shard.index) || shard.index < 0 || shard.index >= SHARD_COUNT) {
    throw new Error('Invalid shard index');
  }
  const args = ['test', '--all-features', '--no-fail-fast', '--locked'];
  // Together these selectors preserve the original --all-targets scope.
  if (shard.index === 0) args.push('--lib', '--bins', '--examples', '--benches');
  for (const target of shard.targets) args.push('--test', target);
  if (shard.index !== 0 && shard.targets.length === 0) throw new Error('Empty integration shard');
  return args;
}

function main() {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], {
    encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
  }));
  const root = metadata.packages.find(pkg => pkg.name === 'reel');
  if (!root || metadata.workspace_default_members.length !== 1 || metadata.workspace_default_members[0] !== root.id) {
    throw new Error('Default workspace test scope changed; update shard coverage explicitly');
  }
  const targets = root.targets.filter(target => target.kind.includes('test')).map(target => target.name);
  const shards = planShards(targets);
  if (process.argv[2] === '--plan') {
    console.log(JSON.stringify(shards, null, 2));
    return;
  }
  if (!/^[0-2]$/.test(process.argv[2] || '')) throw new Error('Expected shard 0, 1 or 2');
  const shard = shards[Number(process.argv[2])];
  console.log(`Ordinary test shard ${shard.index}: ${shard.targets.length} integration targets`);
  const result = spawnSync('cargo', testArguments(shard), { stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exitCode = result.status === null ? 1 : result.status;
}

module.exports = { SHARD_COUNT, planShards, testArguments };
if (require.main === module) main();
