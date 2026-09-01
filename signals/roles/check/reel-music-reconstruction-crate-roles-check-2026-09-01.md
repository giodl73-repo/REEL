---
skill: roles-check
topic: reel-music-reconstruction-crate
date: 2026-09-01
roles_used: [story-director, animation-director, editor, sound-designer, platform-audience, music-reconstruction-engineer, score-arrangement-director, lyrics-vocal-adaptation-editor, rights-provenance-steward]
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# Roles check: REEL music-reconstruction crate proposal — expanded panel

## Artifact identification

- Artifact: `docs/music-reconstruction-crate-proposal.md`
- Type: architecture and staged implementation proposal
- Domain signals: immutable media, music analysis, sample-exact editing,
  translation underlay, score export, orchestration, external adapters,
  privacy, rights, review evidence, and audiovisual synchronization

## Role selection

- `story-director`: protects upstream canon and the separate authority of
  canonical, performed, and translated text through transformation.
- `animation-director`: tests whether musical timing can drive picture without
  coupling renderers or consumer projects to the new crate.
- `editor`: reviews timebase precision, edit operations, lock semantics,
  reversibility, and implementation order.
- `sound-designer`: reviews decomposition evidence, acoustic continuity,
  performance, stems, score reconstruction, and listening gates.
- `platform-audience`: reviews privacy-safe exchange, offline behavior,
  portability, accessible downstream artifacts, and release separation.
- `music-reconstruction-engineer`: reviews decoded identity, analyzer evidence,
  sample/tick timebases, repair operations, locks, and notation round trips.
- `score-arrangement-director`: reviews composition inheritance, editable score
  utility, orchestration plans, and human recognition gates.
- `lyrics-vocal-adaptation-editor`: reviews text-layer separation, underlay,
  prosody exceptions, performed-word evidence, and language authority.
- `rights-provenance-steward`: reviews source scope, voice consent, licenses,
  egress, private/shareable evidence, and release separation.

All nine installed REEL roles were selected because the proposal spans the
complete source-to-song-to-picture production path rather than a private DSP
helper.

## Story Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Canonical, exact as-performed, and target-language text can diverge for legitimate reasons, but silently promoting one to another would change upstream canon. | P2 | Contract family | Keep independent hash-bound layers and reject implicit substitution. Addressed in the proposal. |
| 2 | A validator or role pass cannot approve a translation, arrangement, candidate, or release. | P2 | Authority and review model | Require separately hashed authority and decision references rather than a status string. Addressed in the proposal. |
| 3 | The transformation chain is narratively coherent only if every later artifact cites the exact corrected model it inherited. | P3 | Decision proposed | Preserve directed lineage across repair, language, and arrangement contracts. |
| 4 | Same-composition English must preserve source order and meaning authority while allowing documented prosodic changes. | P3 | Same-composition language adaptation | Keep wording authority upstream and enumerate every musical divergence in the bilingual receipt. |

## Animation Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Picture editors need stable sample/second/beat cues; reading internal crate structures would create a fragile runtime dependency. | P2 | Adapter boundary | Publish the proposed hash-bound `reel.music-timeline-export.v0.1` sidecar. Addressed in the proposal. |
| 2 | Music work must not mutate episode manifests or silently retime locked picture. | P2 | Picture integration | Make the timeline sidecar reference-only and require the production manifest to opt into it. Addressed in the proposal. |
| 3 | External separators, notation tools, and score renderers have different feasibility constraints. | P3 | Adapter boundary | Require capability disclosure and explicit adapter selection before execution. |
| 4 | A limited-ensemble fixture is more inspectable than an unconstrained style-transfer proof. | P3 | Slice E | Retain a part-attributed score-driven fixture before richer instrument recasting. |

## Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Floating-point seconds cannot safely identify sample-exact edit boundaries or guarantee repeatable joins. | P2 | Timeline and identity rules | Use integer samples, integer ticks, explicit PPQ, half-open ranges, and named rounding. Addressed in the proposal. |
| 2 | Crossfades and preserved tails can alter supposedly locked audio if their full envelopes are not included in the edit range. | P2 | Deterministic repair grammar | Include every transition and tail in the changed envelope and reject lock trespass. Addressed in the proposal. |
| 3 | A repair renderer cannot be trusted until the same pipeline proves neutral reassembly of the source. | P2 | Timeline and identity rules | Make decoded-PCM-equivalent no-op reconstruction the first gate. Addressed in the proposal. |
| 4 | Implementing language adaptation and orchestration before timebase and repair invariants would multiply unstable contracts. | P3 | Implementation sequence | Keep the proposed foundation, repair, model/export, language, arrangement, then generative-adapter order. |

## Sound Designer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Separated stems contain bleed and artifacts and must not be represented as recovered original multitracks. | P2 | Decomposition and corrected model | Label every stem as analyzer evidence with engine provenance, confidence, and mixture-consistency disclosures. Addressed in the proposal. |
| 2 | A general tolerance exception could conceal changes outside the requested repair. | P2 | Timeline and identity rules | Require exact PCM identity for `lock`; use separately named protected regions and versioned profiles for whole-program processing. Addressed in the proposal. |
| 3 | Reverb, ambience, phase, and decay tails are musically structural at edit seams. | P2 | Deterministic repair grammar | Measure and bind tails, ambience, phase, loudness, and spectral continuity, while retaining listening review. |
| 4 | English underlay may require pickups, rests, melismas, or note-duration changes even when the composition is inherited. | P2 | Same-composition language adaptation | Require a prosody-exception ledger and bind every divergence to the source model and authority review. Addressed in the proposal. |
| 5 | A checklist of preserved melody/harmony/form cannot by itself prove recognizable musical or emotional identity. | P3 | Score export and re-orchestration | Keep technical comparison separate from mandatory human A/B listening. |

## Platform and Audience

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | A path-free receipt may still leak a private title, work identifier, asset hash, prompt, or reviewer identity. | P2 | Adapter boundary | Split exact local receipts from separately generated privacy-safe receipts that omit all listed identity-bearing fields. Addressed in the proposal. |
| 2 | Validation and doctor commands must not download models, call providers, or process private media as a side effect. | P2 | Adapter boundary | Keep all diagnostics read-only and make execution a separate explicit command. |
| 3 | Cross-platform hash drift would make receipts unreliable for collaborators on Windows and Linux; the current v0.2.25 lyric fixture already fails in this Windows worktree because unpinned LF bytes materialize as CRLF. | P2 | Current-system audit and timeline rules | Pin exact fixture bytes, define canonical serialization, and publish cross-platform golden vectors before schema release. Addressed as the first Slice A prerequisite. |
| 4 | Technical validation, consent evidence, candidate selection, and public release are separate audience gates. | P3 | Authority and review model | Preserve distinct states and never infer one from another. |
| 5 | Lyrics, language labels, instrumental/mix-minus outputs, and timing cues will eventually feed captions and accessible delivery mixes. | P3 | Scope | Keep them as explicit exports that existing REEL caption/timeline contracts can consume; do not duplicate delivery logic in `reel-music`. |

## Music Reconstruction Engineer

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Container hashes alone cannot prove unchanged decoded audio after a neutral remux or metadata change. | P2 | Timeline and identity rules | Retain independent file and normalized decoded-PCM identities and prove neutral reassembly before repair. |
| 2 | Stem, beat, and transcription tools produce hypotheses with different confidence and failure modes. | P2 | Decomposition and corrected model | Require per-observation engine/version/model/parameters, source region, confidence, bleed, and uncertainty. |
| 3 | An edit plan is ambiguous until operation order, overlap rules, rounding, complete transition envelopes, and locked ranges are resolved. | P2 | Deterministic repair grammar | Emit and hash a canonical resolved edit decision list before execution. |
| 4 | MIDI or MusicXML export can silently lose pickups, tempo changes, ties, underlay, or expressive timing. | P2 | Score export and re-orchestration | Require editable-source receipts and a model-to-export-to-reimport comparison with explicit loss warnings. |
| 5 | The synthetic repair fixture needs ambience and a decaying tail to exercise real seam risks. | P3 | Slice A | Retain the proposed phrase error, ambience bed, tail, neutral path, and lock-violation failures. |

## Score and Arrangement Director

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | A genre or style prompt is not an adequate representation of composition identity. | P2 | Score export and re-orchestration | Require preserved/developed/replaced/omitted status for form, melody, harmony, bass, pulse, hooks, cadences, and arc. |
| 2 | Printable PDF cannot be the only score authority because it is difficult to correct, transpose, or compare deterministically. | P2 | Score export | Keep MusicXML or another editable score as authority and treat PDF as an external rendered derivative. |
| 3 | Technical inheritance checks cannot determine whether a new arrangement is recognizable or emotionally faithful. | P2 | Slice E | Require human A/B listening and keep its decision separate from automated validation. |
| 4 | A limited ensemble exposes part attribution and arrangement choices more clearly than a dense first proof. | P3 | Slice E | Keep the limited-ensemble fixture before attempting broader instrument recasting. |
| 5 | Rigid quantization could preserve pitches while destroying the source performance's expressive timing. | P3 | Corrected model | Record local tempo, pickups, bends, rubato, and quantization policy with uncertainty. |

## Lyrics and Vocal Adaptation Editor

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Canonical, as-performed, corrected, and target-language words have different evidence and authority. | P2 | Contract family | Keep separately hashed, source-ordered text layers and prohibit implicit promotion between them. |
| 2 | Timing validation cannot authorize a correction or translation. | P2 | Authority and review model | Require an immutable upstream authority and decision reference for every governed text layer. |
| 3 | Exact requested lyrics do not prove what a candidate actually sang. | P2 | Candidate evidence | Require a separately bound performed-word transcription/listening audit before claiming lyric fidelity. |
| 4 | English underlay may legitimately change pickups, rests, durations, melismas, stress, or cadence. | P2 | Same-composition language adaptation | Require a prosody-exception ledger that names every musical change and its review owner. |
| 5 | Pronunciation guidance is performance evidence, not permission to standardize a speaker's language or erase regional identity. | P3 | Language adaptation | Preserve pronunciation notes as reviewed annotations under the actual language/performance authority. |

## Rights and Provenance Steward

| # | Finding | Severity | Section | Recommendation |
|---|---|---|---|---|
| 1 | Text, composition, performance, arrangement, selection, and release may be governed by different authorities. | P2 | Authority and review model | Preserve distinct authority namespaces and hash-bound decision evidence for every governed layer. |
| 2 | Lyric or manuscript rights do not automatically grant recording, voice-model, or performance permission. | P2 | Adapter boundary | Require operation-, identity-, runtime-, audience-, retention-, and reuse-scoped voice evidence before execution. |
| 3 | A diagnostic that installs, downloads, calls a provider, or processes media would create hidden egress and consent risk. | P2 | Adapter boundary | Keep validation and doctor commands side-effect-free; require a separate explicit execution command. |
| 4 | Even lyric-free/path-free receipts can expose a private project through titles, IDs, hashes, prompts, or reviewer identities. | P2 | Privacy-safe receipts | Keep exact local evidence separate from redacted shareable receipts and verify the redaction contract. |
| 5 | Technical success cannot collapse listening, selection, private delivery, and public release into one status. | P2 | Acceptance gates | Represent and validate each gate separately without inferring progression. |

## Synthesis

Roles reviewed: 9
P1 blockers: 0 | P2 issues: 30 design conditions incorporated | P3 notes: 12

Verdict: **APPROVED-WITH-CONDITIONS**

Top finding: a music-reconstruction result must remain a hash-bound derivative
with exact source, timebase, changed-region, and authority boundaries; neither
machine evidence nor a role simulation can approve its creative truth.

Cross-role consensus: keep `reel-music` provider-neutral, keep the `reel` CLI as
the only process/integration facade, and require explicit evidence at every
transition from source to repair, translation, score, arrangement, and picture.

The conditions move with Slice A: implementation must prove canonical hashes on
Windows/Linux, neutral decoded-PCM reassembly, exact lock enforcement, strict
authority references, privacy-safe receipt redaction, and a loss-visible
notation round trip before the foundation schema can be called complete.

## Amendments applied

1. Added a machine-visible authority model that distinguishes text,
   composition, performance, arrangement, consent, selection, and release, and
   requires separately hashed decision artifacts.
2. Tightened identity and editing rules: canonical cross-platform
   serialization, exact PCM locks by default, named tolerance profiles only for
   non-lock whole-program processing, and full transition/tail envelopes.
3. Added a privacy-safe receipt boundary and the hash-bound
   `reel.music-timeline-export.v0.1` sidecar for picture synchronization without
   leaking private song identity or coupling consumer runtimes.
