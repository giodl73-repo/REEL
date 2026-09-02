---
skill: roles-check
topic: sonic-asset-library-v0318
date: 2026-09-02
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.3.18 sonic asset library roles check

## Artifact identification

- Type: strict catalog/request/resolution schemas, deterministic resolver,
  validator, manifest materializer, path-free receipts, CLI, documentation,
  synthetic tests, and cross-platform FFmpeg CI proof.
- Domain: sound-effects reuse, exact and approved-pool routing, immutable source
  identity, PCM geometry, authority/license/lineage evidence, privacy, and D/M/E
  delivery.
- Reviewed paths: `src/sonic_assets.rs`, CLI wiring, CI, synthetic tests, and
  `docs/sonic-asset-library-v0.3.18.md`.

## Role selection

- Sound Designer: checks sonic identity, sync/loop usefulness, role routing,
  listening boundaries, and stem delivery.
- Editor: checks stable event binding, sync evidence, deterministic repetition,
  and manifest integration.
- Rights and Provenance Steward: checks authority states, license gates,
  immutable lineage, local-path privacy, and approval separation.
- Platform and Audience: checks review variants, mono/small-speaker delivery,
  portability, and consumer usability.
- Music Reconstruction Engineer: checks hashes, sample geometry, deterministic
  selection, stale evidence rejection, and neutral use of the existing renderer.

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Stable asset/variant IDs and explicit exact selection stop a hero vehicle or signature effect from changing between renders. | P3 | request semantics | Keep exact selection mandatory for identity-bearing and synchronization-critical events. |
| 2 | Loop regions and named sample sync markers are verified against source sample count but are not automatically applied as edits. | P3 | catalog geometry | Let authored event timing consume these markers through a separately declared compile policy. |
| 3 | Materialized sources enter ordinary effect/ambience roles, so existing D/M/E, no-score, mono, and small-speaker behavior is preserved rather than forked. | P3 | renderer integration | Retain the ordinary audio graph as the sole rendering authority. |
| 4 | Technical resolution does not evaluate perspective, motion, historical plausibility, emotional fit, or dramatic salience. | P3 | authority boundary | Require Giovanni/listening review of each exact production selection. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Requests bind asset intent directly to unique existing audio-event IDs and reject missing events at materialization. | P3 | event binding | Preserve event IDs through editorial revisions or issue an impact/re-resolution task. |
| 2 | Exact sample counts and named sync markers can anchor approach, stop, idle, and departure timing without relying on filenames. | P3 | timing evidence | Record actual contact/action frame choices in the consumer manifest, not the library. |
| 3 | Approved-pool choice is deterministic from catalog hash, pool/version, request ID, and selection key; no runtime randomness changes a cut. | P3 | pool resolver | Version pool membership whenever editorially visible repetition policy changes. |
| 4 | Materialization writes a new local manifest and refuses overwrite, preserving the authored source manifest for diff and rollback. | P3 | materialization | Keep resolved manifests derivative and rebuild them after source-manifest changes. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Reviewed production and pool states require a separate authority-receipt hash; candidates, diagnostics, and superseded assets fail closed. | P3 | authority state | Keep actual human decisions in consumer-owned authority records. |
| 2 | Production resolution rejects a license record that does not explicitly permit production use, while fixture-only admission requires an explicit engineering-fixture request. | P3 | license gate | Add jurisdiction/term/attribution fields only when a real library license requires them. |
| 3 | Catalog/request/resolution/source/authority/lineage hashes are independently bound, and recheck touches no network, provider, generator, or upload path. | P3 | provenance | Keep model acquisition and generation outside resolver validation. |
| 4 | Local resolution contains paths and is non-shareable; the receipt omits paths/filenames and states that it neither selects creative output nor grants approval. Logical IDs may still be sensitive. | P3 | privacy | Apply consumer disclosure policy before sharing even the path-free receipt. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | The end-to-end fixture emits full mix, pre-master D/M/E, no-score, mono, and small-speaker review outputs from one materialized manifest. | P3 | review delivery | Preserve all variants for production adoption proofs. |
| 2 | PCM WAV parsing and pure-Rust resolution run on Windows and Linux, and CI executes the real FFmpeg D/M/E proof on both. | P3 | portability | Keep synthetic media only in public CI. |
| 3 | The contract allows explicit mono or stereo geometry rather than silently flattening meaningful spatial effects. | P3 | channel policy | Express point-source mono requirements in each consumer request/profile. |
| 4 | A successful technical packet says nothing about phone audibility, recognizability, caption needs, or audience appropriateness. | P3 | review limits | Pair receipts with actual target-device listening review. |

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Source bytes and SHA-256 are checked before RIFF/WAVE PCM geometry is measured, preventing catalog metadata from substituting for the actual signal file. | P3 | source verification | Add a decoded-signal hash only if non-PCM containers are later admitted. |
| 2 | Sample rate, integer bit depth, channels, samples per channel, loop bounds, and sync-marker bounds use integers and exact comparisons. | P3 | timebase | Preserve sample-domain identity throughout future trim/cache features. |
| 3 | Independent check re-resolves from current catalog/request/source bytes and byte-compares canonical packet semantics, rejecting stale policy, source, and packet changes. | P3 | checker | Keep checker inputs explicit rather than trusting an unsigned local receipt alone. |
| 4 | Materialization rechecks the full packet before substitution, and the existing renderer separately binds the resolved manifest and source hashes. | P3 | transitive lineage | Add a consumer proof that connects both receipt hashes to its project authority record. |

## Synthesis

Roles reviewed: 5
P1 blockers: 0  |  P2 issues: 0  |  P3 notes: 20

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: the resolver deterministically executes and proves already-declared
sound authority while correctly failing closed on diagnostic, candidate,
superseded, stale, unlicensed, ambiguous, or tampered inputs.

Cross-role consensus: REEL now has an appropriate generic sound-library grammar,
but a technically valid source is not necessarily the right dramatic sound.
Exact production selection and listening approval remain external human acts.

## Amendments

1. Add consumer-side impact analysis that identifies events affected by an
   authority or pool-version change before re-resolution.
2. Add a separately authored compile policy if sync markers or loop regions are
   to drive event offsets automatically; never infer that behavior.
3. Exercise one real consumer catalog with a fail-closed diagnostic asset and,
   after human selection/license clearance, one successful selected canary.
