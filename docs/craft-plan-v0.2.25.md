# Cross-department craft plans — v0.2.25

The additive `reel.craft-plan.v0.1` sidecar gives a moving-image production one
reviewable place to preserve choices made across directing, cinematography,
production design, costume, hair/makeup, performance, editing, sound, score,
animation/VFX, accessibility, and provenance.

REEL organizes and routes these choices. It does not design a costume, establish
cultural authenticity, direct a performer, approve a reconstruction, or decide
which scene deserves music.

Every present department declares:

- its intent and accountable owner;
- planned, in-progress, ready-for-review, or blocked status;
- exact source-evidence, asset, and continuity references;
- an explicit human-review gate, separate from department workflow status.

An approved or changes-requested human gate requires both a reviewer identity
and review reference. Pending and not-required gates cannot carry review
identity. REEL does not infer approval from completeness, ownership, rendering,
or a simulated role review.

## Continuity and craft decisions

Continuity states explicitly record age, wardrobe, hair, hero props, location
zone, time of day, lighting source, screen direction, and reconstruction
disclosure. Screen direction and reconstruction disclosure use controlled
values rather than unchecked prose. States may share a `match_group`; every
craft dimension in that group must agree exactly.

Editorial decisions record `cut_reason`, `eye_trace`, `sound_bridge`, a
renderer-neutral `protected_hold.duration_ms`, and `movement_motivation`.
Animation/VFX requirements record
layers, depth, occlusion, reflections, particles, interaction contacts, cleanup,
evidence, continuity, and asset references.

Editorial and VFX records carry explicit department routing. That routing—not a
hard-coded guess about what a cinematographer or sound designer needs—controls
department-packet inclusion.

## Commands

```powershell
cargo run -- craft-validate `
  manifests/fixtures/craft-plan/three-period-memoir.yaml

cargo run -- craft-coverage `
  manifests/fixtures/craft-plan/three-period-memoir.yaml --output json

cargo run -- department-packet `
  manifests/fixtures/craft-plan/three-period-memoir.yaml costume `
  --output-path target/costume-department.json
```

`craft-coverage` reports which of the twelve departments are present or missing,
workflow status, pending human gates, blocked departments, referenced and
unreferenced registry entries, and structural completeness. Its contract always
sets `artistic_quality_assessed: false`. Structural completeness is neither
approval nor evidence that the creative work is good.

`department-packet` publishes an atomic `reel.department-packet.v0.1` JSON file.
It contains only the selected department state and the periods, evidence, assets,
continuity states, editorial decisions, and VFX requirements explicitly routed
or referenced by that department. It refuses to overwrite an existing packet.

As of v0.2.26, every evidence and asset record also declares `distribution` as
`internal-only`, `approval-required`, or `shareable`. Internal packets remain
simple. An external packet refuses internal-only records and requires an
explicit `--approval-reference` when any selected record requires approval.
External recipients can verify immutable bytes without learning a local path:

```powershell
cargo run -- department-packet plan.yaml costume --distribution external `
  --approval-reference review-001 --output-path target/costume.json
cargo run -- department-packet-receipt target/costume.json `
  --output-path target/costume.receipt.json
cargo run -- department-packet-check target/costume.receipt.json target/costume.json
```

## Sanitized fixture

`manifests/fixtures/craft-plan/three-period-memoir.yaml` is entirely fictional.
It uses generic identities, fixture-only references, invented locations, and
explicit reconstruction disclosure. It contains no BERTICA manuscript text,
real identity, private address, or claim of cultural authenticity.
