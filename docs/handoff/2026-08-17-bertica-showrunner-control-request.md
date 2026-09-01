# BERTICA request — series-level showrunner control

Date: 2026-08-17
Requested REEL target: v0.2.23 or next compatible minor
Reference production: private five-season, thirty-nine-film memoir adaptation
Reference artifact: `C:/src/bertica/production/series/showrunner/showrunner-control-board-v1.yaml`

Implementation status: **implemented in REEL CLI v0.2.23**. The native contract,
five commands, sanitized fixtures, integrity tests and advisory audits are
documented in `docs/showrunner-control-v0.2.23.md`. BERTICA migration and human
creative review remain upstream responsibilities.

## Why REEL needs this layer

`reel.series.v0.1` correctly composes episode manifests and validates source,
runtime, continuity, privacy, review, speaker and release state. Runtime planning
adds useful component budgets and drift audits. Those contracts answer whether
the production units are complete, compatible and executable.

They do not answer whether a season works as an audience experience. A valid
series can still:

- repeat the same episode function three times;
- reveal a character or mystery too early;
- give the younger viewpoint knowledge available only to the adult narrator;
- cluster grief, spectacle, historical explanation or expensive production;
- end several films without a meaningful invitation forward; or
- possess strong individual episodes but no repeatable series engine.

This is a genuine layer above manifests and runtime—not more fields inside every
shot. REEL should support it without trying to generate or decide emotional
truth.

## Proposed contract

Add `reel.showrunner.v0.1`, a provider-neutral planning sidecar that binds to one
existing `reel.series.v0.1` file by path and SHA-256. It references season and
episode IDs; it does not duplicate scripts, child manifests, narration cues,
source ranges or release approvals.

Suggested top-level shape:

```yaml
schema: reel.showrunner.v0.1
showrunner_id: example-v1
series:
  path: series.yaml
  sha256: <exact-series-hash>
engine:
  promise: <human-authored>
  default_movements: [threshold, action, consequence, afterimage]
  allowed_breaks: [compact-afterpiece, unresolved-testimony]
audience_contract:
  assumed_knowledge: <human-authored>
  no_foreknowledge: true
policies:
  max_adjacent_same_primary_tone: 2
  max_adjacent_scale:
    historical-public: 2
seasons:
  - id: S1
    action: make
    audience_job: <human-authored>
    thematic_proposition: <human-authored>
    thematic_counterforce: <human-authored>
    finale_delivery: <human-authored>
episodes:
  - id: S1E01
    function: season-premiere
    function_family: premiere
    dramatic_question: <human-authored>
    pressure: <human-authored>
    consequential_action: <human-authored>
    narrator_distance: inherited-testimony
    revelations:
      - thread: household-belonging
        state: opened
    tonal_position:
      primary: threshold-mystery
      secondary: human-recognition
      intensity: 3
    ending_invitation:
      mode: opened-world
      statement: <human-authored>
    production_scale: household
revelation_threads:
  - id: central-character
    ordered_steps:
      - episode_id: S1E01
        state: present-but-unsolved
```

All creative strings remain editorial assertions owned upstream. A specific
human-readable function may sit beside a normalized `function_family` so audits
do not mistake thirty-nine evocative labels for thirty-nine unrelated episode
jobs. Likewise, primary tone, ending tone and intensity must be structured audit
indices rather than parsed from prose. Enumerations should be extensible through
declared vocabularies rather than hardcoded to BERTICA's particular themes.

## Commands

```powershell
reel showrunner-validate showrunner.yaml --output json
reel showrunner-audit showrunner.yaml --output json
reel showrunner-revelation-map showrunner.yaml --output json
reel showrunner-rhythm-audit showrunner.yaml --output json
reel showrunner-review-queue showrunner.yaml --output json
```

`showrunner-audit` should be the combined report. The narrower commands make
individual findings inspectable and testable.

## P1 — integrity validation

Validation should fail when:

1. the bound series path, schema, ID or SHA-256 does not match;
2. a referenced season or episode does not exist;
3. an episode is duplicated, omitted when full coverage is declared, or placed
   outside its series order;
4. a revelation step points backward or violates an explicit prerequisite;
5. a required field or declared vocabulary value is missing;
6. a policy is malformed or impossible; or
7. a report claims human approval not present in the bound series review state.

Creative weakness must remain an audit finding, not a schema error.

## P1 — episode-function and rhythm audit

Report, without automatically rewriting:

- adjacent identical or equivalent functions beyond configurable limits;
- repeated high-intensity or darkest-passage positions without a declared
  counter-movement;
- missing premiere, midpoint/turn, pre-finale or finale functions only when the
  showrunner file declares those expectations;
- tonal monotony and abrupt tonal transitions lacking a declared bridge;
- consecutive high-scale episodes and season-level scale concentration;
- optional high-complexity production-load clusters when a project supplies
  real location, role, crowd and asset estimates;
- a finale whose declared delivery does not answer the season's audience job;
- an episode with no consequential action or no ending invitation; and
- an allowed engine break that is used but not declared.

Do not assume every series needs three acts, a villain, a cliffhanger, identical
runtime, or an upbeat release after grief.

## P1 — revelation and viewpoint audit

Support ordered revelation threads with optional prerequisites and intentional
unknowns. Report:

- a reveal used before it is opened;
- a thread that disappears without a declared dormancy or closure;
- younger/immediate narrator distance paired with later knowledge when the
  project forbids foreknowledge;
- repeated re-exposition after a thread is already established;
- finale resolution of a thread explicitly marked `remain-open`; and
- historical or verified-context language presented as direct memory when
  declared memory layers differ.

REEL should validate declarations and sequence. It must not infer whether a
memoir statement is historically true.

## P2 — production-scale portfolio

Connect each showrunner episode to existing runtime component budgets and report:

- intimate/household/community/travel/spectacle/historical-public cadence;
- expensive-scale clusters;
- repeated location/cast/component load when supplied as optional estimates;
- opportunities for planned asset reuse; and
- mismatch between a compact episode and an unnecessarily large production
  design assumption.

These are scheduling signals, never automatic recommendations to cut story.

## P2 — review packet

Generate a readable Markdown/JSON packet containing:

- series engine and allowed exceptions;
- season action, audience job, counterforce and finale delivery;
- one row per episode with function, question, revelation, tone, invitation and
  scale;
- revelation timelines;
- adjacency/rhythm findings; and
- distinct open findings for each named human reviewer already present in the
  series contract.

The tool must never synthesize a named reviewer's opinion or convert an AI role
finding into approval.

## Compatibility and ownership boundary

- `reel.series.v0.1`, child manifests and existing commands remain unchanged.
- The showrunner sidecar binds by hash and references IDs; no runtime dependency
  flows back to the story repository.
- Upstream projects own canon, memoir truth, theme, questions, revelation intent
  and human decisions.
- REEL owns schema integrity, sequence analysis, configurable audits, reports and
  propagation into episode composition packets.
- The schema must work for fiction, documentary, memoir, sports series and other
  episodic formats; BERTICA is the reference fixture, not the hardcoded model.

## Acceptance fixture

Use a sanitized fixture of at least two seasons and six episodes that proves:

1. full episode binding and deterministic ordering;
2. a stale series hash is rejected;
3. a reveal-before-prerequisite is rejected;
4. three adjacent identical tones produce a warning under a configured maximum
   of two;
5. an allowed compact afterpiece does not produce a false structural failure;
6. a finale mismatch is reported but does not invalidate the file;
7. younger-viewpoint foreknowledge is reported;
8. JSON reports are stable and machine-readable; and
9. no private reference path or creative source text enters a provider package.

## BERTICA's immediate use

The current BERTICA board covers all 39 active episodes and three cross-series
revelation threads: Abuelo, Bertica and public history. Until REEL implements
this contract, BERTICA will validate its own board locally and treat the file as
upstream editorial planning. When the feature lands, migrate by reference and
hash; do not copy BERTICA's prose into REEL fixtures.
