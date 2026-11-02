# Producer review: Tig music note authoring

Scope: provider-neutral captured-model note APIs, an explicit Tig source adapter,
and a private CLI. Existing local-file underlay and note workflows keep their
relative-path checks. No renderer, model inference, approval or cache lifecycle
policy changes.

Music reconstruction and score/arrangement checks require exact input/model
locks, preserved unchanged notes and dependency names, bounded clocks/layout,
and unreviewed changed-note provenance. The adapter consumes one explicit model;
it cannot claim that referenced analyses, recordings or musical decisions were
validated. The original local source must remain unchanged.

Rights/provenance checks require draft status and cleared candidate decisions,
preserved old historical streams and exact predecessor status, private operational
files, count-only stdout, no network/provider call, and no implicit publication,
Git adoption, custody or eviction. Producer checks do not grant independent or
creative approval.

The initial native fixture requested cooperative-only reads and correctly refused
mandatory path use. The fixture now explicitly requests `prevent-mutation`;
the engine protection gate is unchanged. Verification covers captured-byte
staleness and limits, unchanged dependencies, current/unrelated/stale asset locks,
cache/Git destination refusal, history/status preservation and the actual CLI.
Local results and hosted qualification are recorded separately; this review does
not assert completed music-lane cutover or full musical dependency validation.

Local verification passes all 62 music-core tests, four native Windows Tig/CLI
cases and strict all-target/all-feature Clippy. The native cases also reject
proposals for retained package-member definitions without explicit authoring
permission; readonly export alone does not promote their semantic contract.
Formatting and whitespace checks pass. Hosted integration and actual project
exercise remain separate from these local results.

The authoring adapter now consumes the existing verified byte reader directly.
It rehashes the bounded captured bytes and retains requested reader protection
through use, without asking for a renderer's physical-path capability. The same
four cases compile on Linux and Windows; the latest Windows run and strict
all-target/all-feature Clippy pass. Cooperative readers remain explicitly
cooperative. Hosted Linux behavior and final integration remain separate checks.
The earlier actual-project exercise retains its original source/runtime pins.
