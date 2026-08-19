---
skill: roles-check
topic: reel-provider-attempt-roadmap-v0.2.37
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.37 provider-attempt roadmap roles check

## Artifact identification

- Type: competitive roadmap and proposed evidence slice
- Scope: provider-attempt identity, crash-safe resume planning, exact output
  capture, reproducibility classification, cost and latency evidence, C2PA
  preservation, OTIO interchange, and content-addressed rebuilds
- Working owner systems: BERTICA owns story, prompts, likeness, approval, and
  release; KARTS owns player art, casting, rights, and publication; ICELINES
  owns hockey facts and selector semantics; generation providers own execution
  and provider-native job state.
- REEL contribution: portable technical contracts around those systems,
  without becoming a provider, production tracker, NLE, rights authority, or
  publication system.
- Human boundary: this review evaluates the roadmap as a technical production
  contract. It does not approve prompts, generated media, performers, music,
  rights, publication, or release.

## Competitive thesis

Production trackers retain mutable workflow state but do not produce portable
content-addressed evidence. Timeline interchange systems preserve editorial
structure but do not attest rendering or generation. Render managers dispatch
work but leave reproducibility and artifact identity to each job. Generation
providers expose temporary job IDs and expiring outputs, while seeds do not
guarantee deterministic regeneration.

REEL can lead by providing an audit-grade, provider-neutral envelope:

1. immutable intent and attempt identity;
2. exact input, resolved-configuration, and output evidence;
3. explicit retry and parent-child lineage;
4. honest reproducibility grades;
5. deterministic resume planning and verification;
6. strict separation between technical evidence and human authority.

The thesis is disproved if the first production slice cannot delete a real
consumer-maintained provider ledger or resume workaround.

## Role selection

- Animation Director: provider configuration, exact output capture, continuity,
  regeneration, and reproducibility affect visual feasibility.
- Editor: retries, alternates, selection, stale cuts, and resume behavior affect
  revision control and editorial lineage.
- Platform and Audience: expiring outputs, output profiles, C2PA, cost, latency,
  and technical readiness affect delivery fitness.
- Story Director: manifests, shot identity, prompts, and provider attempts must
  preserve source ownership and narrative authority.
- Sound Designer: provider-attempt evidence must be media-generic and preserve
  cue, stem, timing, and listening-review boundaries.

## Animation Director

| # | Finding | Severity | Recommendation |
|---|---|---|---|
| 1 | A model name and seed do not establish reproducibility; cloud providers may route models or change implementations. | P2 | Record requested policy separately from resolved provider/model/configuration evidence and report `exact-byte-reuse`, `deterministic-local-replay`, or `best-effort-provider-replay`. |
| 2 | Provider output URLs may expire before production review. | P2 | Require immediate exact-byte capture and media probing before an attempt can become technically materialized. |
| 3 | A resumed attempt must not silently replace an existing candidate or selected asset. | P2 | Make every retry a new immutable attempt linked to its parent; retain promotion as a separate human-led operation. |
| 4 | Provider-native execution details differ too widely for a universal execution API in V1. | P3 | Standardize evidence ingestion and resume decisions first; keep provider execution in consumer-owned adapters. |

## Editor

| # | Finding | Severity | Recommendation |
|---|---|---|---|
| 1 | Resuming by provider job ID alone can attach output to a changed cut. | P2 | Bind intent and attempt evidence to the exact production-manifest hash, operation identity, shot or cue identity, and generation-plan hash. |
| 2 | Retry, remix, retake, extension, and exact rerun are editorially different operations. | P2 | Require an explicit operation kind and parent edge rather than treating every provider call as an interchangeable retry. |
| 3 | Automatically choosing the latest successful attempt would erase editorial alternatives. | P2 | Report eligible outputs but never select, promote, or supersede one implicitly. |
| 4 | Resume planning is useful only when its decision is explainable. | P3 | Emit deterministic reasons such as `reuse-captured`, `poll-existing`, `download-expiring`, `retry-terminal`, or `blocked-stale-input`. |

## Platform and Audience

| # | Finding | Severity | Recommendation |
|---|---|---|---|
| 1 | Cost, latency, provenance, and delivery readiness are independent axes. | P2 | Keep quote, realized cost, queue/runtime, C2PA status, and technical output conformance as separate fields and findings. |
| 2 | C2PA evidence does not grant rights or publication authority. | P2 | Preserve and verify credentials when present, while retaining separate consumer-owned rights and release gates. |
| 3 | Provider payloads can contain credentials, prompts, private URLs, or sensitive failure details. | P2 | Keep raw provider packets local; portable reports contain hashes, normalized classifications, bounded opaque identifiers, and no secrets or prompt text. |
| 4 | Cost accounting is valuable but provider-specific actual charges are frequently unavailable. | P3 | Represent quote, reservation, actual charge, currency or credits, and unavailable values explicitly; never infer missing actuals. |

## Story Director

| # | Finding | Severity | Recommendation |
|---|---|---|---|
| 1 | A global provider job ledger without story scope could reuse output after the source or shot meaning changes. | P2 | Scope intent to manifest hash plus stable operation and shot/cue identity; reject stale bindings. |
| 2 | Portable evidence does not need prompt text to establish technical lineage. | P2 | Bind prompt and source material by exact hash while leaving prose, private rationale, and source canon in the owner repository. |
| 3 | Technical success must not be described as creative approval. | P2 | Use lifecycle terms such as `submitted`, `running`, `completed`, `failed`, `captured`, and `verified`; retain `selected` and `approved` only in the separate promotion ledger. |
| 4 | A provider-neutral contract must not flatten owner-specific semantics. | P3 | Keep operation purpose and domain policy in consumer-owned inputs while REEL validates generic identity and evidence invariants. |

## Sound Designer

| # | Finding | Severity | Recommendation |
|---|---|---|---|
| 1 | A picture-only attempt contract would immediately duplicate later voice, music, ambience, and effects ledgers. | P2 | Make attempt identity media-generic, with typed media facts and optional shot, cue, stem, or timed-span bindings. |
| 2 | Audio generation may yield technically valid files with unusable performance, timing, or mix. | P2 | Keep listening review and take selection separate from capture and technical verification. |
| 3 | Retakes must preserve exact cue or time-span lineage. | P2 | Bind retake attempts to the source attempt, operation kind, manifest hash, and exact authored span. |
| 4 | No-score and silence are intentional production states, not failed generation attempts. | P3 | Do not require provider attempts for explicitly authored no-score or silence variants. |

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 15 | P3 notes: 5

Verdict: **APPROVED-WITH-CONDITIONS**

The competitive direction is sound, but the original roadmap grouped too many
independent capabilities into one release. The five roles agree that v0.2.37
should prove a narrow provider-attempt evidence and resume contract before
adding signed approvals, OTIO interchange, generalized build graphs, or broad
cost policy.

## Revised v0.2.37 evidence slice

### Normative V1

1. Accept a strict, sanitized provider-attempt input produced by an
   owner-controlled adapter.
2. Bind the attempt to an immutable intent, production-manifest hash,
   operation kind, shot/cue/span identity when applicable, generation-plan
   hash, requested-policy hash, and resolved-configuration hash.
3. Model immutable lifecycle transitions and explicit parent-child retry,
   retake, remix, or extension lineage.
4. Capture terminal output bytes immediately and verify hashes, bytes, media
   type, dimensions or duration, and relevant stream facts.
5. Emit a path-free portable receipt and provide an independent checker.
6. Produce a deterministic resume decision without calling a provider.
7. Classify replay honestly as exact-byte reuse, deterministic local replay,
   or best-effort provider replay.

### V1 boundaries

- No provider API calls, credentials, polling, downloading, or billing.
- No prompt text, private source text, raw provider payloads, private URLs, or
  local cache paths in portable output.
- No automatic selection, promotion, approval, supersession, publication, or
  release.
- No workflow assignments, comments, schedules, dashboards, or tracker state.
- No signed approvals, C2PA authoring, OTIO import/export, or generalized
  build-graph engine in the first slice.

### Required proof

- One BERTICA S1E01 still-generation intent.
- One completed and captured attempt.
- One structured terminal failure followed by an explicit retry.
- One stale-manifest rejection.
- One resume report that chooses reuse without provider execution.
- One independent receipt check after moving the portable receipt and captured
  artifact.

### Deletion gate

The slice succeeds only if BERTICA can delete or retire its hand-maintained
provider-attempt ledger and at least one resume/reconciliation workaround while
retaining prompts, provider execution, creative review, rights, and release
authority.

## Deferred competitive roadmap

1. Signed approval attestations and C2PA verification/preservation.
2. Quote, reservation, realized-cost, queue, runtime, and download accounting.
3. OTIO import/export for editorial timeline interchange.
4. Content-addressed dependency graphs, changed-only rebuilds, and exact
   rollback.
5. Optional tracker and render-farm adapters that exchange evidence without
   making their databases authoritative inside REEL.

## Human authority boundary

An attempt that is completed, captured, verified, reproducible, inexpensive,
fast, or provenance-bearing is not thereby selected, creatively approved,
rights-cleared, publishable, or releasable. Those decisions remain explicit
human actions in the owning production system.
