const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync, spawnSync } = require('node:child_process');
const { classifyFiles, changedFiles, classifyEvent } = require('./docs-only.cjs');

test('README and documentation prose/images use the fast path', () => {
  assert.equal(classifyFiles(['README.md']), true);
  assert.equal(classifyFiles(['docs/guide.md', 'docs/nested/diagram.svg', 'docs/reading notes.txt']), true);
});

test('source, build, CI and unknown changes always need full qualification', () => {
  for (const file of ['src/lib.rs', 'Cargo.lock', '.github/workflows/ci.yml',
    '.github/scripts/docs-only.cjs', 'browser/assembly/app.js', 'tools/helper.py',
    'vendor/tig/README.md', 'docs/example.json', 'docs/example.yaml', 'docs/example.js']) {
    assert.equal(classifyFiles(['README.md', file]), false, file);
  }
  assert.equal(classifyFiles([]), false);
});

test('manual and new-branch events conservatively run full qualification', () => {
  assert.equal(classifyEvent('workflow_dispatch', {}), false);
  assert.equal(classifyEvent('push', { before: '0'.repeat(40) }), false);
  assert.throws(() => classifyEvent('pull_request', {}));
});

test('actual required-check gates reject classifier and qualification failures', () => {
  const ci = fs.readFileSync(path.join(__dirname, '../workflows/ci.yml'), 'utf8').replace(/\r/g, '');
  const gateSection = ci.slice(0, ci.indexOf('\n  rust-tests:'));
  const gates = [...gateSection.matchAll(/        run: \|\n((?:          [^\n]*\n)+)/g)]
    .map(match => match[1].replace(/^ {10}/gm, ''));
  assert.equal(gates.length, 2);
  const bash = process.platform === 'win32'
    ? path.join(process.env.ProgramFiles || 'C:/Program Files', 'Git/bin/bash.exe') : 'bash';
  const cases = [
    ['success', 'true', 'skipped', true],
    ['success', 'false', 'success', true],
    ['failure', 'true', 'skipped', false],
    ['cancelled', '', 'skipped', false],
    ['success', '', 'skipped', false],
    ['success', 'false', 'failure', false],
    ['success', 'false', 'skipped', false],
    ['success', 'true', 'failure', false],
  ];
  for (const [index, script] of gates.entries()) {
    for (const [CLASSIFICATION, DOCS_ONLY, QUALIFICATION, passes] of cases) {
      const result = spawnSync(bash, ['-e', '-c', script], {
        env: { ...process.env, CLASSIFICATION, DOCS_ONLY, QUALIFICATION, HTTP_QUALIFICATION: QUALIFICATION }, encoding: 'utf8',
      });
      assert.ifError(result.error);
      assert.equal(result.status === 0, passes, `${CLASSIFICATION}/${DOCS_ONLY}/${QUALIFICATION}`);
    }
    if (index === 0) {
      for (const HTTP_QUALIFICATION of ['failure', 'cancelled', 'skipped', '']) {
        const result = spawnSync(bash, ['-e', '-c', script], {
          env: { ...process.env, CLASSIFICATION: 'success', DOCS_ONLY: 'false',
            QUALIFICATION: 'success', HTTP_QUALIFICATION }, encoding: 'utf8',
        });
        assert.ifError(result.error);
        assert.notEqual(result.status, 0, `HTTP failure admitted: ${HTTP_QUALIFICATION}`);
      }
    }
  }
});

test('Git diff retains deletion/rename paths and unusual documentation names', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'reel-docs-ci-'));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  const commit = () => {
    git('add', '-A');
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture');
    return git('rev-parse', 'HEAD').trim();
  };
  try {
    git('init', '-q');
    fs.mkdirSync(path.join(root, 'src'));
    fs.mkdirSync(path.join(root, 'docs'));
    fs.writeFileSync(path.join(root, 'src/lib.rs'), 'original source\n');
    fs.writeFileSync(path.join(root, 'README.md'), 'original readme\n');
    const base = commit();
    fs.writeFileSync(path.join(root, 'docs/Spanish guía.md'), 'documentation\n');
    const docs = commit();
    assert.equal(classifyFiles(changedFiles(base, docs, root, true)), true);
    assert.equal(classifyEvent('push', { before: base, after: docs }, root), true);
    assert.equal(classifyEvent('pull_request', {
      pull_request: { base: { sha: base }, head: { sha: docs } },
    }, root), true);
    fs.renameSync(path.join(root, 'src/lib.rs'), path.join(root, 'docs/moved.md'));
    const moved = commit();
    const files = changedFiles(docs, moved, root);
    assert.ok(files.includes('src/lib.rs'));
    assert.ok(files.includes('docs/moved.md'));
    assert.equal(classifyFiles(files), false);
    assert.equal(classifyEvent('push', { before: docs, after: moved }, root), false);
    // A later base-branch source edit must not become part of a docs-only PR.
    git('checkout', '-q', '-b', 'base-advance', base);
    fs.writeFileSync(path.join(root, 'src/lib.rs'), 'base advanced\n');
    const advancedBase = commit();
    assert.equal(classifyFiles(changedFiles(advancedBase, docs, root, true)), true);
    assert.throws(() => changedFiles('missing-ref', docs, root));
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
