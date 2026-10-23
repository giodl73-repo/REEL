# REEL review roles

The files in this directory are production and infrastructure review checklists
used to examine REEL artifacts and tools from distinct perspectives. They do not represent actual
people, approvals, or authority to publish.

For substantive production changes, select the roles whose focus intersects
the artifact, preserve disagreements in the review record, and keep human
approval separate from simulated findings. A passing role review complements
tests and receipt validation; it does not replace them.

Current lenses cover animation direction, editing, platform/audience fit, sound
design, story direction, music reconstruction, score and arrangement, lyric and
vocal adaptation, rights/provenance, and the technical panel below.

## Asset infrastructure panel

- [Asset Graph and Version Integrity](parliament/asset-graph-integrity.md)
- [Storage Custody and Recovery](parliament/storage-custody-recovery.md)
- [Registry Concurrency and Evidence Integrity](parliament/registry-evidence-concurrency.md)

Use all three for shared graph/lifecycle architecture or cross-cutting custody
changes; use the applicable subset for a bounded implementation. Add Rights and
Provenance for evidence privacy, authority, source custody or exchange, and the
relevant craft roles when native authoring/production semantics change.

Record the reviewed commit/paths, role revisions, severity, owner, correction,
actual validation and remaining gates. Distinguish a corrected design from a
passed implementation fixture. Identify producer-applied versus independent
review; roles are checklists, not simulated people or authority to spawn agents.

## Core production panel

- [Story Director](story-director.md)
- [Animation Director](animation-director.md)
- [Editor](editor.md)
- [Sound Designer](sound-designer.md)
- [Platform and Audience](platform-audience.md)

## Music transformation panel

- [Music Reconstruction Engineer](music-reconstruction-engineer.md)
- [Score and Arrangement Director](score-arrangement-director.md)
- [Lyrics and Vocal Adaptation Editor](lyrics-vocal-adaptation-editor.md)
- [Rights and Provenance Steward](rights-provenance-steward.md)

Use the smallest complete panel for the artifact:

- source decomposition or deterministic repair requires music reconstruction,
  sound design, editing, and rights/provenance;
- lyric correction or language adaptation additionally requires lyric/vocal
  adaptation and the actual upstream text or translation authority;
- notation, score reconstruction, or re-orchestration requires score/arrangement
  direction plus music reconstruction and sound design; and
- private exchange or candidate delivery requires rights/provenance and
  platform/audience review.

These roles test evidence and production contracts. They do not determine who
owns a composition, approve a translation or performance, grant voice consent,
select a creative master, or authorize publication. Those decisions remain
with the actual authorities named by the producing project and must be recorded
in separate, immutable decision artifacts.
