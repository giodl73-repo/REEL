# REEL music-reconstruction crate proposal

Status: Slices A, B, C1, C2, interchange intake, evidence comparison, semantic import, and governed model drafting implemented through REEL v0.3.5
Date: 2026-09-01
Proposed CLI milestone: v0.2.26 foundation, followed by additive repair,
language-adaptation, and arrangement slices

## Decision proposed

Add a library crate named `reel-music` under `crates/reel-music/`. Keep the
existing `reel` package and binary as the public CLI and integration facade.
The crate owns provider-neutral song-reconstruction contracts, validation,
timeline arithmetic, transformation planning, and privacy-safe evidence. It
does not bundle a separator, transcription model, synthesis engine, notation
renderer, or FFmpeg.

`reel-music` is deliberately broader than `reel-audio-repair`: the durable
domain is a song's recoverable musical model and its governed transformations,
not one repair implementation. The crate must support this progression:

```text
immutable source
  -> decomposition evidence
  -> corrected editable music model
  -> minimal deterministic repair
  -> same-composition language performance
  -> score-driven arrangement and re-orchestration
```

Each arrow produces a new, hash-bound derivative. No stage overwrites its
input, declares a machine estimate authoritative, or infers human approval.

## Authority and review model

REEL does not decide who owns a lyric, translation, performance, composition,
or arrangement. Each governed layer declares an upstream authority namespace,
artifact identifier, content hash, status, and required review roles. An
approval reference must identify a separate immutable decision artifact and
its hash; a status string or completed role simulation cannot stand in for it.

The generic contracts distinguish at least these authorities: source/canonical
text, exact as-performed text, target-language text, composition, performance,
arrangement, consent, candidate selection, and release. One person or project
may hold several authorities, but REEL retains the distinctions. Missing
authority or decision evidence remains an explicit gate, not an inferred denial
or approval.

REEL role routing for this contract family is:

- source, analysis, and repair: `music-reconstruction-engineer`,
  `sound-designer`, `editor`, and `rights-provenance-steward`;
- target-language underlay or performed-word evidence: add
  `lyrics-vocal-adaptation-editor` and the actual upstream language authority;
- score export or re-orchestration: add `score-arrangement-director` and the
  actual composer/music authority;
- music-to-picture timeline exchange: add `animation-director`; and
- private/shareable delivery: add `platform-audience`.

`story-director` remains required when a song transformation also adapts
narrative source material or affects a work's story structure.

## Why a crate boundary is warranted

REEL is currently one `reel` package with domain modules under `src/`, including
`song`, `audio_quality`, `comparison`, `selection_lock`, and the production
timeline. The root library also contains general manifest and renderer
orchestration. Music reconstruction introduces a coherent set of contracts
that must share sample-exact time arithmetic and invariants but should not make
the general video library depend on DSP or model runtimes.

The proposed workspace shape is:

```text
reel/
  Cargo.toml                 # package + workspace; depends on reel-music
  src/                       # CLI, video manifests, renderer orchestration
  crates/reel-music/
    Cargo.toml
    src/
      lib.rs
      source.rs              # immutable media identity and rights/egress claims
      time.rs                # samples, ticks, beats, bars, and rounding rules
      analysis.rs            # stems/features/alignment evidence and confidence
      model.rs               # corrected editable composition/performance model
      repair.rs              # reversible operations and protected regions
      language.rs            # source/target lyric layers and underlay changes
      arrangement.rs         # score-driven orchestration transformations
      evidence.rs            # hashes, receipts, checks, and approval separation
      adapter.rs             # capability/request types, never engine execution
```

The root manifest should add `crates/reel-music` as a workspace member, use the
edition-2024 workspace resolver, and add a path dependency from `reel` to
`reel-music`. The new crate should begin with the existing baseline libraries
already used by REEL: Serde, YAML/JSON, SHA-256, and explicit error handling.
Heavyweight or native DSP dependencies require a later reviewed decision.

### Current-system audit evidence

- `Cargo.toml` defines one root package and an empty `[workspace]`, so the new
  member can be added without splitting the existing CLI package.
- `src/song.rs` already proves strict exact-lyric, engine-plan, consent, local-
  egress, and receipt patterns that should migrate only after byte-compatible
  tests exist.
- The root CLI delegates its `song-*` commands directly into that module, which
  is the compatibility seam for a later `reel-music::generation` move.
- The current Windows checkout exposes a portability defect in the v0.2.25
  sanitized fixture: `song.yaml` records the SHA-256 of LF lyric bytes, while
  Git materializes `lyrics.txt` as CRLF because that file has no pinned EOL
  attribute. Both song-generation test targets consequently fail first on the
  lyric hash. Pinning fixture bytes and adding Windows/Linux hash vectors is a
  prerequisite maintenance change, not evidence against exact-byte hashing.

## Contract family

Do not create one mutable mega-manifest. Use separate versioned contracts whose
hash references form a directed lineage.

| Contract | Purpose | Must establish | Must not claim |
|---|---|---|---|
| `reel.music-source.v0.1` | Freeze the input | File SHA-256, decoded-PCM fingerprint, stream facts, rights/egress declarations | That the mix is separable or correct |
| `reel.music-analysis.v0.1` | Record decomposition and observations | Analyzer/engine provenance, stem hashes, tempo/form hypotheses, aligned events, confidence and uncertainty | That stems are original multitracks or estimates are approved |
| `reel.music-model.v0.1` | Hold the corrected editable song model | Meter/tempo map, form, lyrics layers, notes, harmony, bass, rhythm, hooks, expressive timing, source evidence | That automatic transcription is authoritative |
| `reel.music-repair.v0.1` | Express a bounded correction | Input/model hashes, ordered operations, changed envelope, locked outside regions, tail policy, validation thresholds | That rendering or listening approval occurred |
| `reel.music-language.v0.1` | Fit a target language to the same composition | Source and target text hashes, underlay, melody inheritance, prosody exceptions, accompaniment/model lineage | That REEL authored or approved a translation |
| `reel.music-arrangement.v0.1` | Re-score or re-orchestrate from the model | Preserved/developed/replaced elements, instrumentation map, score/stem targets, comparison criteria | That a style label alone preserves musical identity |
| `reel.music-candidate-evidence.v0.1` | Bind rendered results to plans | Output hashes, outside-region checks, acoustic facts, lyric/listening status, review gates | Selection, release, or creative approval |
| `reel.music-timeline-export.v0.1` | Connect music to picture without coupling runtimes | Model/plan hash, sample-to-second map, tempo/form map, named beats/cues, duration and rounding policy | That picture timing or an edit is approved |

Canonical lyrics, exact as-sung lyrics, and target-language lyrics are separate
layers with separate hashes and authority. A tool may align or compare them; it
may not silently promote one layer into another.

## Timeline and identity rules

Seconds represented as floating point are inadequate for repair boundaries.
Every audio contract uses a declared timebase:

- integer sample positions against a declared sample rate for acoustic edits;
- integer musical ticks against a declared PPQ for score events;
- explicit mappings between samples, ticks, beats, bars, and form sections;
- a documented rounding mode whenever a mapping is not exact; and
- monotonic ordered ranges with half-open interval semantics.

Strict YAML is the review input, but receipts hash a specified canonical JSON
serialization: UTF-8, schema-defined field names, sorted map keys, preserved
array order, normalized finite numbers, and normalized content references.
Machine-local paths are resolved only for local validation and never become
portable identity. Golden test vectors must prove identical canonical bytes and
hashes on Windows and Linux before a schema is released.

Two identities are retained:

1. the source-file SHA-256, which proves the exact container input; and
2. a normalized decoded-PCM fingerprint, which permits neutral-reassembly and
   outside-region identity checks despite container metadata differences.

The first gate is always a no-op/neutral plan. It must reconstruct the declared
timeline and prove decoded PCM equality before any repair is accepted.

## Deterministic repair grammar

The initial operation vocabulary is deliberately small:

- `keep`, `cut`, `insert`, `replace`, `repeat`, and `move` sample ranges;
- `crossfade` with an explicit curve and duration;
- `preserve-tail` with source and destination envelopes;
- `match-gain` and `match-eq` with measured targets and tolerances;
- `extend-bars` through a referenced musical range; and
- `lock` for every unaffected range.

Operations are ordered and non-overlapping unless an explicit operation group
defines composition order. Crossfades, ambience fills, and reverb tails belong
to the changed envelope and may not trespass into a locked range. Renderers
must emit a resolved edit decision list before executing it.

Candidate validation includes duration, channel/sample-rate consistency,
outside-region decoded-PCM identity, boundary discontinuity, phase correlation,
loudness, spectral discontinuity, ambience, and tail continuity. Exact decoded-
PCM identity is the default for every locked region. A whole-program operation
such as mastering may instead declare a separately named protected region with
a versioned measurement profile and bounded tolerances; it may not call that
region `lock`. Profiles and observed values are recorded in private evidence,
while the shareable receipt records only pass/fail categories. Passing these
checks is technical evidence, not proof that the words, performance, or musical
feeling are correct.

## Decomposition and corrected model

Separators and analyzers are optional adapters. Their outputs are evidence:

- synchronized stems with mixture-consistency and bleed disclosures;
- tempo, meter, beat, bar, and form hypotheses;
- vocal/as-sung word, syllable, breath, note, bend, and cadence alignment;
- melody, harmony, bass, rhythm, hook, and instrumentation hypotheses; and
- per-observation engine identity, version, model revision, parameters,
  confidence, and source region.

The corrected model is a separate human-reviewable derivative. Corrections do
not mutate analyzer output. Every corrected event cites its evidence and records
whether it is `observed`, `inferred`, or `human-corrected`. Unknowns remain
unknown rather than being filled for convenience.

## Same-composition language adaptation

The language contract inherits an exact accompaniment or mix-minus, tempo/form
map, melody guide, and corrected source model. It adds the approved target text,
syllable/note underlay, pronunciation and stress notes, and a prosody-exception
ledger. Each exception declares whether it changes note onset, duration, pitch,
melisma, rest, pickup, or phrase boundary and who must review it.

The source-language wording and performance remain immutable. REEL validates
timing and lineage but does not translate, approve wording, or claim semantic
equivalence. A bilingual comparison receipt binds both performances to the
same inherited musical source and names every approved musical divergence.

## Score export and re-orchestration

The corrected model can request MIDI, editable MusicXML, a lead-sheet input,
tempo/form maps, click/count-in, melody/bass/harmony guides, and stems. Export
receipts bind the model hash, exporter/version, quantization policy, warnings,
and round-trip comparison. Printable PDF is rendered by an external notation
adapter from the editable MusicXML; PDF is never the sole score authority.

Arrangement plans classify every important element as `preserve`, `develop`,
`replace`, or `omit`: form, meter/pulse, melody, harmony, bass motion, rhythmic
cells, hooks, cadences, and emotional arc. Instrumentation is a reviewed map,
not a hard-coded cultural checklist. A limited ensemble is the first synthetic
fixture because part attribution and comparison are inspectable. Recognition
and emotional identity remain human listening judgments.

## Adapter boundary

`reel-music` owns adapter-neutral request/capability types and verifies returned
evidence. The root `reel` CLI owns process orchestration and explicit adapter
selection. Initial adapter classes are:

- decode/probe and deterministic render through the existing FFmpeg boundary;
- separation, beat/form analysis, transcription, and notation export as
  externally supplied evidence;
- ACE or another re-singing/repaint engine only after a deterministic repair
  plan exists; and
- later score renderers and instrument libraries behind explicit adapters.

No doctor or validation command downloads a model, installs a tool, opens a
network connection, or processes private media. Execution commands must be
explicit and must preserve engine, model, seed, license, network, and egress
provenance. Private requests may contain paths or text. Local receipts bind
exact artifact, input, and plan hashes. Separately generated shareable receipts
omit paths, filenames, titles, song/work identifiers, lyrics, local asset
hashes, prompts, authority names, and review reasons; they carry privacy-safe
aggregate facts and a verifier-facing binding that does not expose the private
lineage.

`reel.music-timeline-export.v0.1` is the only direct picture-integration surface
in the foundation. It exposes sample/second timing, tempo and form regions, and
named musical cues as a hash-bound sidecar that a production manifest may
reference. It neither rewrites a video manifest nor creates a runtime
dependency from a consumer project to `reel-music`.

## CLI surface

The `reel` binary remains the only user-facing executable. Add commands in
small implementation slices rather than exposing unfinished stages:

```text
reel music-source-validate
reel music-neutral-plan
reel music-neutral-check
reel music-analysis-validate
reel music-model-validate
reel music-repair-plan
reel music-repair-check
reel music-language-validate
reel music-arrangement-validate
reel music-export-plan
reel music-candidate-check
```

Existing `song-*` v0.2.25 commands remain compatible. After the foundation is
stable, their contract implementation can move from `src/song.rs` into
`reel-music::generation` without changing command names, schema identifiers, or
packet bytes. Migration requires golden compatibility tests before deletion of
the root module implementation.

## Implementation sequence

### Slice A — crate and source/repair planning foundation

- Pin the existing song-generation fixture's text bytes across worktrees and
  restore its v0.2.25 tests before moving any implementation.
- Create the workspace crate and path dependency.
- Implement exact timebase types, canonical hashing helpers, strict schema
  dispatch, source validation, neutral planning/checking, and repair-plan
  validation.
- Add one short synthetic PCM fixture containing a repeated/mistaken phrase,
  ambience, and a decaying tail; include no BERTICA text or audio.
- Prove a no-op decoded-PCM round trip and reject edits that touch locked ranges.
- Publish canonical-serialization test vectors that match on Windows and Linux.

### Slice B — deterministic repair rendering

- Compile the edit plan to a resolved decision list. **Implemented v0.2.27.**
- Invoke the existing FFmpeg boundary from the root CLI. **Implemented v0.2.27.**
- Render the synthetic one-phrase repair and verify boundaries, tails, duration,
  acoustic continuity, and outside-region identity.
  **Implemented v0.2.27 with generated periodic raw PCM.**
- Produce a private artifact plus a path-free evidence contract and comparison
  input. **Private artifact and evidence implemented v0.2.27; comparison input
  remains additive future work.**

### Slice C — corrected model and notation round trip

- Validate externally produced analysis evidence. **Implemented in C1 v0.2.28.**
- Build a small human-corrected synthetic music model. **Implemented in C1
  v0.2.28 with explicit event provenance and fixture-only correction evidence.**
- Export MIDI and MusicXML through explicit adapters and re-import enough of
  both to detect lost form, notes, lyrics, or tempo changes. **Implemented in
  C2 v0.3.1.**
- Generate an audible guide from the corrected model and bind it to the export
  receipt. **Implemented in C2 v0.3.1.**
- Admit outputs from existing tools without executing or replacing them.
  **Implemented in C3 v0.3.2.**
- Compare competing admitted evidence and emit explicit human selection and
  correction queues without automatic ranking. **Implemented in C4 v0.3.3.**
- Validate adapter-normalized semantic events with exact integer time mappings
  and promote them into analysis observations with retained import lineage.
  **Implemented in C5 v0.3.4.**
- Require complete mapped/omitted/unknown observation dispositions and verify
  model evidence citations bidirectionally. **Implemented in C6 v0.3.5.**

### Slice D — same-music second-language proof

- Add a synthetic source/target lyric pair and underlay exceptions.
- Bind a second-language vocal to the same accompaniment and model.
- Produce a bilingual comparison receipt without asserting translation quality.

### Slice E — score-driven limited-ensemble arrangement

- Map the synthetic model into a deliberately small ensemble.
- Validate preserved/developed/replaced musical elements.
- Render an audible comparison and record human listening as the recognition
  gate.

### Slice F — optional generative performance adapters

- Connect bounded re-singing/repaint to the existing song-engine provenance
  boundary.
- Reject candidates whose outside-region evidence, lyrics audit, or boundary
  checks fail.
- Keep every failed candidate and reason distinct from the selected repair.

## Acceptance gates

The first complete fixture is accepted only when it demonstrates all of the
following without consumer-private material:

1. immutable source and decoded-PCM identities;
2. neutral reassembly before editing;
3. one on-grid minimal phrase repair with locked outside audio;
4. separate analyzer evidence and human-corrected model;
5. one same-accompaniment target-language underlay with declared exceptions;
6. editable MIDI/MusicXML and an audible round trip;
7. one score-driven limited-ensemble arrangement;
8. deterministic failure diagnostics for stale hashes, invalid timebases,
   overlaps, lock violations, tail trespass, and missing review gates; and
9. private artifacts plus privacy-safe receipts that never imply selection,
   consent, approval, or release; and
10. a hash-bound music-timeline sidecar that can drive picture synchronization
    without exposing private song identity.

## Explicit non-goals for the foundation

- No private BERTICA audio, lyrics, paths, or project-specific judgments in
  REEL fixtures or tests.
- No automatic download, model installation, or remote provider call.
- No claim that source separation recreates original multitracks.
- No automatic correction of canonical or translated lyrics.
- No universal music-information-retrieval engine inside the core crate.
- No instrument recasting before the corrected composition model and minimal
  repair are stable.
- No public-release or creative-approval state inferred from validation.

## Review and implementation decision

Slice A passed its contract simulation and expanded implementation role review
with no open P1 or P2 finding. Later slices still require their own bounded
implementation and review gates. Human music, translation, rights, and release
authorities remain external to REEL's role simulation and automated validation.
