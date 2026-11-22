# Media expansion: test and evidence matrix

This matrix is normative for the [phased PR plan](media-expansion-pr-plan.md).
All rows are future requirements, not completed tests. M00 must turn the core
rows into exact fixture/test IDs and runnable commands against the chosen source
interfaces; M11/M13 do the same for playback/interaction before their runtime PRs.

## Evidence contract for each PR

Record each applicable requirement as:

| Field | Required content |
| --- | --- |
| Identity | Planning ID, requirement ID, exact source revision and fixture version |
| Invocation | Exact command, crate/feature selection and tool/runtime pins; explicit local inputs |
| Expected result | Observable output, diagnostic or state transition; numeric limits and tolerances where relevant |
| Oracle | Hand-calculated golden values, independently evaluated decoded media, contract validator or explicit human decision; say which |
| Execution | Platform, actual result, exit status, elapsed time and evidence location; distinguish pass, fail and not run |
| Review | Applicable role findings and their disposition; exact bound edition/take/script/join/mix/runtime identities for production review |
| Remaining gate | Listening/device review, selection, delivery and adoption recorded independently; no inherited approval |

Commands must exercise behavior, not merely create output or compare two copies
of the producer's report. Tests may share parsing/contract libraries with a
checker, but the checker must reopen/rederive the observable property; it cannot
call the producer and accept its own reported result as the oracle.

### Pin before executing

- M00 fixes the short core fixtures' sample rate/channel layout, decoded format,
  durations, half-open interval convention, rounding rule, protected regions,
  gain/envelope policy and output/scratch/time limits. Use exact integer-sample
  equality for controlled lossless fixtures. A no-processing path must preserve
  the selected PCM exactly; transformations have an independently computed
  expected result under the declared arithmetic.
- If a delivery codec is lossy, declare its decoded comparison metric, tolerance,
  encoder delay/padding policy and runtime scope before its tests. Do not use
  encoded-byte equality across platforms or quietly relax thresholds after a
  failure. Source-container identity and decoded-signal identity remain separate.
- Pin peak/clipping, loudness and intelligibility review targets appropriate to
  the fixture brief. A controlled signal proves placement/math, not pronunciation
  or natural dialogue. Do not infer speech quality from a tone test.
- Run core contract/compiler/checker fixtures on Windows and Linux. Media tests
  exercise every platform advertised for the new path with explicit decoder/
  encoder pins. An unavailable runtime is **not run** and leaves that platform
  unqualified. Device-specific players may support a narrower declared matrix;
  document it before implementing them rather than claiming general portability.
- M11 fixes loop discontinuity policy, permitted clock drift, command-to-state
  latency, restart deadline and resource caps for its one device/runtime profile.
  M13 fixes input ordering, transition timing, seek/re-entry and persistence rules.
  Record units, measurement method and numeric tolerances; no runtime gate closes
  on an undefined "appropriate" gap, "in sync" or "fast enough".

## Per-PR expectations

| PR | Required positive evidence / oracle | Required rejection or change evidence | Remaining human or scope boundary |
| --- | --- | --- | --- |
| M00 | Source-backed reuse map; all four edition briefs; exact core test IDs, commands and thresholds, with hand-calculated clock examples | Identify unsupported paths and contract gaps rather than labeling a nearby feature equivalent | Source/text authority and the recording/listening scope must be named; no generation |
| M01 | Versioned contract examples for four editions and a multi-speaker multi-cue module; validate stable IDs and distinct text/evidence layers | Duplicate/unknown IDs, unresolved context, stale identity, invalid intervals, undeclared overlap, missing required scope declarations | Schema validity proves declarations, not truth, consent or translation accuracy |
| M02 | Hand-calculated sample plans for distinct EN/ES durations; recorded silence plus added gap; continuation and speaker-change joins; deterministic repeated compile | Missing/extra selected input, marker outside take, trim crossing a protected boundary, unsupported rate, overflow/negative/out-of-range placement | Declared dependency checks cannot infer whether a summary changed the facts |
| M03 | Zero-processing PCM identity plus independently calculated gap/overlap/fade cases; sample counts and full changed/unchanged intervals; bounded execution | Changed admitted bytes, incompatible format, existing output, resource limit, injected interruption; no final candidate package on failure | No real speech quality claim; inspect recovery/temporary-output behavior explicitly |
| M04 | Reopen actual media and rederive placements/counts; four matching transcript/chapter packages and speaker roles | Audio/text/chapter/role tampering; altered source/placement plus recomputed report hashes; out-of-bounds or misordered text intervals | Word fidelity and language naturalness still need per-language listening |
| M05 | Spanish-detail v2 changes only Spanish technical consumed recipe; compare all four recipe IDs and review bindings; reopen retained v1 | Missing retained bytes/runtime, stale review after a changed take/join/mix; no automatic restore; a changed aggregate index must not invalidate unrelated recipes | Retention proof covers this fixture and admitted interfaces, not every cache or global custody |
| M06 | Baseline four-edition technical pack plus within-module two-speaker fixture; all core negatives traced to tests | Reordered/omitted detail creates a separately checked adjacency; changed input invalidates the affected review | Human rubric below; synthetic engineering completion leaves listening not run |
| M07 | Independently evaluated short dialogue/bed/effect mix; event placement per language; declared ducking/envelopes/tails and peak checks | Stale event, invalid gain/envelope, undeclared overlap, missing scope; bed revision changes mix but preserves take IDs | Listen for masking and transition quality; no source separation or arrangement claim |
| M08 | Small EN/ES reading package with source-ordered text, chapter index, exact audio/print bindings and earlier-version reopening | Wrong language/text hash, missing chapter, reordered text, dangling visual reference; changed source invalidates consuming package | Language/source decisions and text-access review; no implied EPUB/store support |
| M09 | Editable-score fixture includes pickup and repeat traversal; hand-derived score-to-sample anchors and current exports bound to the model | Ambiguous repeated passage, wrong timebase/rounding, stale score/audio/export; reject unsupported traversal instead of guessing | Musical/notation/underlay usefulness needs actual musical review; PDF cannot replace model authority |
| M10 | Two authored transitions with distinct EN/ES phrase times; decoded frame boundary checks, caption timing and audio-only path without picture inputs | Stale alignment, missing visual/alpha source, contradictory sample/frame boundary; new take changes only consuming outputs | Visual continuity/legibility and language review; automatic checks do not decide style quality |
| M11 | [Pinned installation profile](offline-installation-v1.md), numeric clocks/limits, explicit states and normal/error traces; [I01–I09 obligations](reviews/2026-11-21-m11-installation-contract-review.md) | Missing/incompatible source/runtime; invalid loop/control state; interrupted playback and cancelled startup/restart policy | Contract only; device capture and listening procedure written before M12 implementation; no test execution claimed |
| M12 | State traces with fake clock plus actual bounded playback on the declared device; measure loop/drift/control/restart limits | Missing media, device disconnect/runtime mismatch and interruption exercise declared failure/recovery states | Fake-clock pass is not device evidence; do not claim unattended reliability |
| M13 | Finite graph examples with two authored paths, EN/ES policy, deterministic event order and accessible controls | Missing/unreachable node, undeclared cycle, ambiguous simultaneous input, incompatible edition and stale session state | Author/story and audience review; no inferred branches or automatic pedagogical approval |
| M14 | Replay an identical input trace for identical route/selections/media-clock placements within M13's limits; keyboard-only completion of both paths | Input at transition/end, pause/re-entry, allowed language switch, missing branch and invalid persisted state follow explicit policy | Actual text/focus/control usability and device test required for those claims |
| M15 | Compatibility/support matrix links exact checked fixtures, runtimes and review records for every claimed format | Regression probes for old supported fixtures and frozen contexts; identify skipped/unsupported combinations | Unperformed listening/device/adoption gates stay visible; docs checks cannot qualify media |

For rejection cases, assert the documented nonzero status or explicit failure
state and the failing relationship/diagnostic class. Planning rejection must
start no media process. Invalid producer input must publish no final package;
invalid checker input must produce no success report or asset-promotion write.
An interrupted player follows its declared safe state, rather than treating every
playback failure as a compilation error.

## Human review rubric and gate handling

For every actual spoken edition, review the complete programme and each join in
its declared listening context. Record exact reviewed versions, reviewer/authority,
findings and unresolved items in governed evidence. Address:

- **Meaning:** opening purpose and closing conclusion are coherent; omitted detail
  preserves qualifications, references and source truth; no implied endorsement
  or fabricated conversational context.
- **Language/performance:** exact observed wording, uncertainty, pronunciation,
  stress and meaning in each language; translation approval is a distinct decision.
- **Handoffs:** continuation/response rhythm, pauses, recorded breath/tail, changes
  of speaker, explicit overlaps and the separate executive host-to-host adjacency.
- **Mix/access:** intelligible speech, masking/clipping/tails, speaker-role text,
  relevant non-speech information, matching chapters and target-device access.
- **Authority:** declared text/voice/reuse/provider scope and separate listening,
  selection, private-delivery and public-release decisions.

No blocking meaning, word-loss, authority or intelligibility finding may be
closed by a numerical metric alone. A human decision must name its actual scope;
one person's review does not become an invented panel or a second language's
approval. New takes/joins/mix or scripts stale their bound review as applicable.

### Privacy and execution regression checks

Include controlled canary values in fixture-only local paths, prompts, speaker
identity and authority fields. Assert those values are absent from portable
reports and normal CI/error output under the existing privacy-safe contract.
Private debugging evidence stays local; do not populate canaries with real data.
Bind exact private evidence through the admitted interface without exposing it.

Validation/planning tests must assert no provider process, outbound request,
download, generation, storage write or player startup occurs. Where existing
validation legitimately emits a requested local plan/report, allow only those
declared outputs. Use existing injectable ports or controlled stubs; a new network
service or general-purpose test framework is outside this plan.

Each PR records technical, listening/device, selection and adoption states
separately. Missing human evidence permits only the explicitly demonstrated
engineering scope; it never turns into a pass through elapsed time or a green CI.
