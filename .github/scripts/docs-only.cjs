const fs = require('node:fs');
const { execFileSync } = require('node:child_process');

// Keep this an allowlist: examples, schemas, workflows and unknown paths run CI.
function isDocumentation(file) {
  return file === 'README.md' || /^docs\/.+\.(md|txt|rst|png|jpe?g|gif|webp|svg)$/.test(file);
}

function classifyFiles(files) {
  return files.length > 0 && files.every(isDocumentation);
}

function changedFiles(base, head, cwd = process.cwd(), pullRequest = false) {
  const git = (...args) => execFileSync('git', args, {
    cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  });
  const start = pullRequest ? git('merge-base', base, head).trim() : base;
  // No rename collapsing: a source-to-docs move must include the deleted source.
  return git('diff', '--no-renames', '--name-only', '-z', start, head, '--')
    .split('\0').filter(Boolean);
}

function classifyEvent(eventName, event, cwd = process.cwd()) {
  if (eventName === 'pull_request') {
    return classifyFiles(changedFiles(event.pull_request.base.sha, event.pull_request.head.sha, cwd, true));
  } else if (eventName === 'push' && event.before && !/^0+$/.test(event.before)) {
    return classifyFiles(changedFiles(event.before, event.after, cwd));
  }
  return false;
}

function main() {
  const event = JSON.parse(fs.readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const docsOnly = classifyEvent(process.env.GITHUB_EVENT_NAME, event);
  fs.appendFileSync(process.env.GITHUB_OUTPUT, `docs_only=${docsOnly}\n`);
  console.log(docsOnly ? 'Documentation-only change: expensive qualification is skipped.' : 'Full qualification required.');
}

module.exports = { isDocumentation, classifyFiles, changedFiles, classifyEvent };
if (require.main === module) main();
