# Series showrunner control v0.2.23

REEL CLI v0.2.23 adds `reel.showrunner.v0.1`, a planning sidecar above an
existing `reel.series.v0.1` index. It makes cross-episode audience architecture
reviewable without moving canon, scripts, memoir truth or human approval into
REEL.

## Commands

```powershell
reel showrunner-validate showrunner.yaml --output json
reel showrunner-audit showrunner.yaml --output json
reel showrunner-revelation-map showrunner.yaml --output json
reel showrunner-rhythm-audit showrunner.yaml --output json
reel showrunner-review-queue showrunner.yaml --output json
reel showrunner-review-pack showrunner.yaml --output text
reel showrunner-review-pack showrunner.yaml --output text --output-path review.md
```

`showrunner-audit` combines validation, rhythm and revelation reports. The
narrow commands remain useful for CI, dashboards and human review packets.
Every command emits human-readable Markdown for `--output text` and stable JSON
for `--output json`. `showrunner-review-pack` combines the machine audit with
the distinct human-review queue in one non-rendering packet.

## Binding and ownership

The sidecar names an existing series path, exact SHA-256 and optional series ID:

```yaml
schema: reel.showrunner.v0.1
showrunner_id: private-series-v1
coverage: full
series:
  path: ../series/series.yaml
  sha256: <exact-sha256>
  series_id: private-series
```

Paths resolve relative to the showrunner file. Reports expose the series ID and
hash, not the local path. `full` coverage must match every season and episode in
exact series order. `partial` coverage must be a unique ordered subset.

The showrunner command validates bound-series identity and structural order; it
does not replace the deeper child-manifest, coverage, privacy, timing and
release checks in `reel series-validate`. Run both at production gates.

REEL owns schema integrity, deterministic sequence analysis and reports. The
upstream production owns every creative statement. No command generates a
dramatic question, decides a theme, rewrites an episode, certifies historical
truth, simulates a reviewer, or grants release approval.

## Human-authored controls

Each plan defines:

- a repeatable series promise, default movements and allowed engine breaks;
- audience assumptions, memory layers and no-foreknowledge policy;
- project-owned vocabularies;
- season audience jobs, thematic proposition/counterforce and finale delivery;
- episode function and normalized function family;
- dramatic question, pressure and consequential action;
- narrator distance and audience revelation;
- primary/ending tone, intensity and optional transition bridge;
- optional ordered internal tone beats for consequential turns within an episode;
- ending invitation and production scale; and
- ordered revelation threads, prerequisites, closures and knowledge uses.

An optional production-load record can add one-to-five complexity, location and
speaking-role counts, crowd presence, new assets and reusable assets. Projects
that do not yet possess real estimates omit it; REEL never converts a dramatic
scale label into invented cost.

Knowledge uses may include an authored `handoff` description. A legacy
`handoff_declared: true` is accepted only when that description is present, so
the report can show how adult reflection, testimony, captions, picture, or sound
enters an immediate memory rather than recording an unexplained boolean.

Normalized audit fields sit beside expressive project language. This lets a
memoir call an episode a “wonder countermovement” while also identifying it as
the generic `release` function family used for adjacency analysis.

## Integrity failures

Validation rejects:

- an unknown schema, empty required field or malformed coverage mode;
- a stale series hash or mismatched series ID;
- missing, duplicate or out-of-order season/episode coverage;
- undeclared vocabulary values or invalid intensity/policy ranges;
- duplicate or out-of-order revelation steps;
- missing or non-earlier revelation prerequisites;
- a revelation assigned to the wrong episode; and
- an episode using a revelation before that step has opened.

These are contract failures, not aesthetic judgments.

## Advisory rhythm audit

Projects opt into specific limits. The audit can report:

- adjacent repeated function families or primary tones;
- adjacent maximum-intensity episodes;
- production-scale clusters such as three spectacle films in a row;
- missing season-required function families;
- a finale that has not declared delivery of the season audience job;
- an engine break not listed by the plan; and
- a configured abrupt ending-tone to primary-tone transition without a bridge.

The tool does not assume three acts, equal runtime, a villain, cliffhangers, a
midseason turn, or emotional release after grief. A project only receives a
finding for a policy it declared.

## Revelation and viewpoint audit

Threads carry ordered steps with stable IDs. Prerequisites and episode order are
strict. A step may declare `through_episode_id` when a revelation develops
across an interval rather than appearing in one instant; sequential steps may
not overlap. Optional advisory checks surface repeated states, excessive undeclared
dormancy, closure of a `remain_open` thread, lack of closure on a thread expected
to close, and adult/later knowledge used from an immediate narrator distance
without a declared handoff.

REEL checks the declared information architecture. It cannot determine whether
a memory, testimony, allegation, faith experience or historical claim is true.

## Human review queue

The plan declares required reviewer IDs. The queue reads only the bound series'
existing findings and human-review status, then reports each reviewer as open or
approved. Missing findings remain open. It never creates a finding in a named
person's voice or averages disagreement into a synthetic decision.

## Sanitized fixture and tests

`manifests/fixtures/showrunner/showrunner.yaml` exercises all five CLI commands
against the existing sanitized threshold series. The acceptance suite also
constructs a path-free two-season, six-episode fixture and proves:

- deterministic full-coverage and report ordering;
- stale-hash rejection;
- reveal-before-prerequisite and use-before-open rejection;
- configurable repeated-tone warnings;
- allowed engine-break behavior;
- advisory finale mismatch;
- younger-viewpoint later-knowledge findings;
- distinct human review queues; and
- omission of local paths from serialized reports.
