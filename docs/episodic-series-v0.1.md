# Episodic series contract v0.1

`reel.series.v0.1` is the v0.2.1 reference and composition layer over independent
`reel.manifest.v0.2` scene artifacts. It never embeds child shots, narration,
private references, or approval state.

## Commands

```powershell
reel series-validate series.yaml --output json
reel series-plan series.yaml --output json
reel series-coverage series.yaml --output json
reel series-review-queue series.yaml --output json
reel episode-compose series.yaml S2E02 --output-dir composed/S2E02-v1 --output json
```

The validator resolves every child path and verifies its work ID, SHA-256,
accepted timing/review state, required platforms, speakers, privacy state,
source completeness, and duration. Child references cannot repeat anywhere in a
series. Season and episode order, identifiers, canonical ranges, omissions,
poem/prose dependencies, episode totals, and season totals are checked.

An episode cannot be release-ready while a child is untimed, unreviewed,
privacy-blocked, or source-incomplete. Bertica and Herman findings remain
independent entries; neither filenames nor tool output infer consensus or human
approval.

Series defaults carry platform, disclosure, caption, privacy, and shared
continuity policy into an episode packet. Child manifests remain referenced in
that packet, so their explicit overrides are not erased.

## Atomic episode packets

`episode-compose` accepts timed children, verifies the entire series first, and
publishes a new directory atomically. It never edits or retimes a child packet.
The directory contains:

- `manifest.yaml` — ordered child hashes, offsets, protected-pause IDs, inherited
  defaults, and separately attributed production units;
- `captions.srt` — child captions shifted onto one continuous episode timeline;
- `lineage.json` — series hash, child hashes, offsets, and production units;
- `coverage.json` — canonical ranges, disclosed omissions, and production units;
- `duration.json` — child, production-unit, and total duration.

Title cards, bridges, and credits must use `source_kind: production-authored`.
They cannot masquerade as manuscript narration.

The working template is `manifests/templates/episodic-series.yaml`. Automated
acceptance also builds a sanitized five-season slate with ten episodes per
season and continuous coverage from blocks 34 through 4419.
