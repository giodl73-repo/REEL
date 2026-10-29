# Presentation control providers

Problem: retained presentation media could use guarded byte providers while
its adoption manifest and episode control route still required source files.
That prevents an honest source-independent episode cutover.

The additive APIs resolve pinned manifests and bounded controls through explicit
providers. The original decoded source-equivalence algorithm remains shared by
local and cache callers. Episode conform now offers exact parent, scene and
presentation metadata coverage, keeping composition sources distinct from
primary inputs and retained outputs. No new media format, storage index, daemon
or global cache lock is introduced.

## Checklist review

These are applications of the repository's role checklists, not independent
reviewers or human approval.

- Editing: preserve original frame counts, ordered components and timestamp
  checks; do not relabel an opening as a scene to bypass presentation checks.
- Sound: preserve decoded PCM identity, existing excerpt clocks and explicit
  clock-repair evidence. Source equivalence does not approve a performance.
- Rights/provenance: original selected output and receipt subjects remain
  unchanged. A successful check is technical evidence only. Underlying source
  production lineage and publication authority remain separate requirements.

## Qualification

The existing real-media integration case is extended with a cached manifest,
missing original controls/media, exact verification-cache reuse, fresh full
verification, refusal of missing/changed/oversized/traversing manifest pins and
a true presentation-kind ordered-unit parent. Missing component metadata must
refuse before a parent output exists. The extended case passes standalone
(79.40 seconds). All 13 presentation tests pass (158.38 seconds), all seven
ordered-unit tests pass (276.06 seconds), the existing episode conform test
passes (11.21 seconds) and all 13 Tig episode tests pass (40.50 seconds).
All-feature library tests pass 209 with nine explicitly ignored; all-target,
all-feature Clippy passes with warnings denied. Hosted qualification is pending.

Remaining cutover work includes presentation currency in Tig composition job
admission, bounded parent job execution and complete child resource handoff.
This change does not claim those milestones or allow eviction.
