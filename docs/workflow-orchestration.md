---
work: workflow-orchestration
stage: manifest
format: production-workflow
style: renderer-neutral
source_scenario: project-owned
author: producer
rubric_version: v0.1
created: 2026-11-17
updated: 2026-11-17
sources: [Cargo.toml, .roles/ROLE.md, .pitfall/PITFALL.md]
---

# REEL workflow orchestration

## Current implementation and boundary

`reel-workflow` is an additive task/result contract prototype, not a production
runner. Its library checks a bounded DAG, creates hash-bound private prompt
packets, validates direct/handoff result envelopes, checks returned bytes and
compares exact logical-attempt replays. Its CLI provides `check-plan`,
`export-task` and `check-result`. Export requires a new destination; failed
writes remain for diagnosis. Control files are limited to 1 MiB, plans to 128
tasks, each prompt to 64 KiB and task inputs to 128. These are contract bounds,
not Tig actor or changed-path capacity claims.

Current v2 task/result contracts use named multi-artifact sets shared with
`reel-publication`. Film/comic edition metadata has its own read-only checker;
see [publication contracts](publication.md). The single-output v1 prototype is
not silently reinterpreted.

No API/proxy, ElevenLabs recording, image-generation, GitHub issue, native
storage admission, retained-owner acknowledgement, selection application or
scratch retirement adapter is installed. `check-result` never marks a task
complete. Direct mode currently has a library adapter boundary and synthetic
tests, not a live endpoint. Handoff is private packet construction/checking,
not yet qualified resumable export/import against managed resources.

Example local contract commands (synthetic files only until W1/W2 qualify):

```text
reel-workflow check-plan PLAN
reel-workflow export-task PLAN TASK NEW_PACKET [DEPENDENCY_DESCRIPTORS]
reel-workflow check-result PACKET RESULT PAYLOAD_BINDINGS
```

Packets contain complete supplied prompts and exact input descriptors, not
automatically assembled references or downloaded media. Dependency descriptors
are planning inputs; their hashes are not proof that the referenced bytes exist,
belong to the named producer or have retained protection. Real export/dispatch
must pass the qualified source/retention adapter before using these contracts.
Result checking establishes byte identity, declared type and byte allowance;
it does not decode media, validate a browser package, evaluate semantic quality,
authenticate provider/model assertions or establish rights.

## Ownership

| Owner | Contract |
| --- | --- |
| Project repositories | Canon, selected source text/recordings, references, style/pacing policy, actual decisions and private issue routing. Melena is one consumer alongside game/demo/documentary projects. |
| reel-workflow | Durable task/attempt orchestration, direct API/proxy/subprocess and manual handoff modes, dependency invalidation, findings and fix routing, structured adapter errors. |
| reel-publication | Film/comic edition structure, spatial comic page/reading order and named multi-artifact output contracts. No storage publisher, render engine or release authority. |
| reel-assembly | Existing semantic graph, source revisions, shared events, language-local cue/trigger bindings, measured alignment and deterministic timing. |
| reel | Existing compilers, managed producers, browser compositions, exposure sheets, sprites, sound/VFX, review derivatives, complete-film checks and review records. |
| reel-music / reel-print | Existing musical controls/transformations and optional printed derivatives. |
| reel-assets | Existing producer job/capacity/legacy integration, review-ID authority and admitted retention seams. Preserve existing consumers. |
| browser assembly workbench | Session-local diagnostics over native contracts; not a render or release authority. |
| Tig | Exact committed records/objects, guarded native byte operations, existing claims/Index/actor lifecycle, publication and retained-owner protection. No new cache engine. |

## Two modes, one identity and result path

Direct mode calls a declared adapter. Handoff mode issues ready private task
packets and waits for returned envelopes and files. Modes can mix in one graph.
Neither mode auto-selects or approves results. Do not feed returned prose into a
shell command or automatically execute returned browser code.

The task key binds the versioned packet contract, project namespace, exact source
commit, task definition, prompt, language, ordered input descriptors, output
contract and exact dependency result/artifact descriptors. It excludes its own
hash. An unrelated task added to the same snapshot does not invalidate a packet;
a changed source commit conservatively changes keys. Cross-commit reuse requires
separately qualified recipe/input equivalence; it is not inferred here.

Each result names the task key and an explicit attempt ID. Exact replay is
idempotent comparison; changed content/provenance under the same attempt conflicts.
A new generation attempt is explicit. Direct/handoff share validation, with actual
execution mode and available provider/model evidence recorded. Unknown evidence
remains unknown. Failed direct calls are returned without automatic retry.

## Film controls to connect, not duplicate

- Keep format, visual school, performance style and animation technique distinct.
  Realism/theatrical/expressionist directions are project policies, not renderers.
  Defaults may be overridden explicitly per sequence/shot and frozen into inputs.
- Reuse `ScenePolicy` target duration bounds and semantic-cut checks. Shot hold,
  drawing exposure/cadence and delivery frame rate remain distinct. Use exposure
  sheets for multiple drawings per shot and layer-specific motion. Estimate
  generation, storage and render costs before admission; report infeasibility
  instead of silently reducing density. Additional renderer adapters remain
  separately qualified work.
- Reuse shared semantic events and `LanguageEventBinding`, exact selected-take
  word evidence, phrase occurrences, native alignment and phrase-to-sample
  resolution in reel-assembly. Every language compiles its own cue lengths,
  picture spans, score/Sonic/VFX, captions and complete ending. Missing/ambiguous
  triggers block the affected task. Recording changes invalidate alignment and
  downstream timing; they do not automatically rerecord other languages.
- Reuse existing recordings, including retained future-season takes. ElevenLabs
  recording is a new operation with explicit source text, permitted voice/model,
  performance settings, returned evidence and candidates; alignment import alone
  is not generation.
- Browser composition is an existing technique for HTML/CSS/SVG/JavaScript and
  hybrid film units. Freeze all local sources/fonts/assets and the browser build;
  use the REEL frame clock. Review generated code before trusted local execution.
- Reuse the shared source-hash review-ID allocator; never invent a per-worktree
  counter. Logical cel, immutable drawing and on-screen review identity differ.
  Retain language-local frame ranges/sidecars and clean masters. Labeling defaults,
  codecs and cel reuse restrictions come from the producing project.
- Findings bind exact film/version, language, cel/cue, range, actual review modality
  and requested change. Route project defects to project issues and generic defects
  to REEL/Tig using synthetic evidence. Preserve historical issue URLs. Deduplicate
  effect retries. Resolve only after the corrected affected consumers and required
  decisions are evidenced. Minimal rebuild scope does not reduce contextual review.

## Tig integration baseline

Use qualified Rust lifecycle interfaces (or a thin wrapper around them), plus
pinned native byte CLIs. The git-asset front door does not expose the full managed
job/retained-publication lifecycle; do not parse its generic stderr into invented
error classes. Preserve the existing reel-tig-job integration rather than wrapping
legacy builders or adding cache/retention machinery.

Supplied qualification identities, verified/qualified upstream within their scope:

| Surface | Exact identity |
| --- | --- |
| Consumed Tig SDK, 247 files | source-manifest SHA256 `f89d836d82b1a6240fbe77f4fd53b488ed57ceb303111642be53fd668ffe2cfe` |
| EVID-151 combined managed lifecycle source | `1c3d0b7db734bc946c3fd482134cabaac1c023f67ea7d2f19a0bf4c3f63d88e3` |
| EVID-161 git-asset ZIP | `8b1d8fdd7aa579a65e55fd2af0ba29985195cd624caa9573875b518d458bc7b1` |
| EVID-162 multi-pool reader executable | `4e626ecf3523c4073a8face5ecac09fae2eee9e8cf80c2521ec16d8cfa3d06f8` |
| EVID-162 combined source | `efc2a4423e7b7171d06f3de332b49b741286471fe92d8083a229a0c1755446b0` |

The SDK's c62c58c ancestry is not an exact source pin. EVID-151 is source
qualification, not an installed lifecycle distribution. EVID-162's four-store
reader is not incorporated into the existing consumed SDK/job protocol. Machine
locators belong in an owned ignored profile. This prototype does not replace the
older SDK vendored in its committed base or claim any of these new integrations.

Admit against the actual immutable actor table: 16 total actors, not 16 available
workflow slots. Each phase has a 128 changed-path bound. Closing requires a ready
retained actor covering every assigned store. Surface Busy/Budget/Unsupported
without silently allocating actors, chunking transitions or switching protocols.

The production lifecycle to qualify is:

```text
pin committed inputs -> preview declared closure -> accept durable protection
-> prepare owned workspace / issue private handoff -> wait or dispatch
-> ingest unselected candidate -> protect/publish/reopen/acknowledge retained owner
-> record durable task completion -> separately apply approved selection
-> retire owned scratch -> close job protection into retained coverage
```

No automatic protection expiry exists. Waiting, timeout and cancellation are
workflow states, not removal permission. Long manual waits keep input claims.
On uncertain effects, preserve and reconcile the original operation, WorkRef,
activation generation, object pins, source/Index checkpoints and owner ack. Never
replace an uncertain operation with a new identity to make retry appear clean.

## Bounded implementation work packages

### Comic consumer assessment

Read-only inspection of BBJ's current production instructions, comic-page and
layered-source contracts, archive/native-reference preflights, comic/print review
lenses and versioned Rust production tools confirms the same orchestration needs:
exact scripts/reference locks -> layout proposals -> illustration candidates ->
layered art -> editable lettering -> ordered reading proof -> findings/fixes ->
new immutable edition. Direct/handoff modes, task/attempt identity, retained
candidate ingestion and expected-baseline selection are reusable. Existing BBJ
tool versions/author snapshots stay immutable; this assessment runs no BBJ jobs
and changes no active source, selection, server, storage binding or book.

Comics require a distinct production profile, not a film with artificial seconds:

- Identity is publication/issue/page/panel/layer/utterance, alongside exact source
  versions. Distinguish a published comic issue from a defect-tracker ticket.
- Spatial sequence includes panel polygons, reading/balloon order, spreads,
  recto/verso placement and page-turn reveals. Panel count is not a pacing score
  or an animation cel budget. Comic panels adapt into independent film shots/cels
  through an explicit adaptation graph, not an automatic one-panel/one-shot map.
- Separate clean art, character/prop/effect layers, lettering/balloon/tail/text
  sources, fonts and selected reading composites. Reconstructing pixels exactly
  does not prove correct semantic layer ownership or future editability.
- Each language needs exact utterance/text identity, balloon fit, font/glyph,
  reading-order and page/issue proof checks. A translated text layer may require
  geometry changes while shared art stays protected. No speech synthesis is
  required for a static book. Film word clocks apply only to an explicit narrated
  or animated derivative; static semantics use beat/panel relationships.
- Reading PNG/SVG/PDF, editable sources and press-specific outputs have different
  contracts. Require fresh-folder reconstruction of clean art AND new lettering
  composition, not just the selected flattened page. Print readiness additionally
  needs the actual printer contract and physical proof.
- Reuse BBJ's accepted versioned Rust compose/assemble/verify/rebuild/pdf tools
  through a pinned adapter. Its current LFS/native session is not automatically
  migrated to git-asset. Layer packaging still has separate historical adapters;
  do not rerun frozen helpers or claim a complete Rust port.

This exposes concrete extensions before the prototype becomes a production API:
named multi-artifact output contracts (one page can produce independent SVG, PNG,
lettering, clean art and source archive assets), profile-specific validation
capabilities, and pinned deterministic-tool tasks alongside intelligence tasks.
The W0 v2 contracts now share named multi-artifact output sets with
`reel-publication`; film and comic edition structure is separately checkable.
The W0 operation enum still describes intelligence proposals and does not yet
dispatch deterministic comic assembly through managed lifecycle admission. The
optional publication producer now passes the bounded bilingual page/lettering/
book-render, one-line fix and source-unavailable restoration fixture described in
[publication contracts](publication.md). Native retention, stale-handoff integration
and selection application remain separate qualification, without touching an active
BBJ issue or pretending comic art is an animation rig.

| Package | Owner / entry | Exit evidence still required |
| --- | --- | --- |
| W0 task/result contracts | reel-workflow; committed REEL base and synthetic inputs | Bounded DAG/identity checks, direct fake and handoff equivalence, stale/duplicate/corrupt refusal, no-clobber CLI. This is the current slice. |
| W1 resumable manual staging | REEL lifecycle adapter; exact qualified SDK/source profile | Actual accepted protection, exact closure/staging, interrupted export, days-long reopen/cancel and conservative slot return under synthetic fixtures. |
| W2 idempotent external candidate registration | REEL adapter over existing Tig publisher and retained owner | Stable task/attempt/artifact identity; repeated import, failed response, publish-before-ack and recovery; no task completion before retained owner ack. |
| W3 expected-baseline selection application | REEL Git adapter over Tig proposals | Expected-ref compare/apply or checked merge; new-path registration separately admitted; stale main/other writer refuses without overwriting selection. |
| W4 structured integration errors | REEL adapters | Normalize typed SDK observations; unknown I/O/uncertain CLI failure remains NeedsRecovery or Unsupported. Never infer safe retry from exit 1. |
| W5 providers, review and project adoption | REEL + each project | API/proxy/subprocess adapters, explicit image/speech operations, issue dedup/closure, bilingual film fix/isolation, second synthetic project and owned rollback. |

Busy: bounded fresh-check retry. Stale/Conflict: explicit replan preserving the
candidate. NeedsRecovery: reconcile original effects before retry. Missing:
explicit admitted acquisition. Integrity/Denied/Unsupported: stop and diagnose.
Effect retry policy is adapter-owned; it is not a string-to-retry boolean here.

Qualification is principally scoped Windows/NTFS evidence, not universal storage
or producer acceptance. Run targeted locked tests/Clippy/fmt and diff checks for
W0. Later work must use actual synthetic/native lifecycle fixtures, not fabricated
booleans or structural success. Project adoption requires normal operation with
historical source checkouts unavailable and an owned binding rollback.
