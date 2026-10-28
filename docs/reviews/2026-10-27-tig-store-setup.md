# Explicit shared-store setup

Entryb73e3266920293951e9fbca7bb4ab6f6acbb6f13 (REEL212).
Tig WP-003/006/016, REQ-003/004/006/010, SPEC-003/005/008,
IF-005/012, PKG-003/004 consumer adapter. Existing Tig providers and shared REEL
capacity adapter remain authoritative; no new cache engine or schema dialect.

Add explicit setup/check-stores commands. Setup accepts existing state parent,
active capacity root and shared coordination; admits bounded database/rollback
and bootstrap allowance before creating an owned directory. Create metadata and
empty source journal, finish/reopen/read back, then publish a new private bindings
file. Preserve the directory only after successful no-overwrite publication.
Reuse the existing owned-directory cleanup/recovery guard. Check opens only named
existing stores with trusted minimum checkpoints, never creates missing providers.

No setup on run/capture/resume, no production invocation, cache init, discovery,
daemon, source/job adoption, transfer or removal. Explicit setup is once per shared
project authority, not once per worktree/job. Admission is cooperative and covers
this operation, not a permanent journal quota. An abrupt crash before publication
can retain an unselected owned directory/stale capacity record; full orphan
recovery commands remain open and no automatic deletion is authorized here.

L0 fmt/locked lint/isolation. L1 successful create/check/reopen, foreign checkpoints,
capacity/malformed/duplicate output refusal, concurrent same-output publication,
owned-only cleanup and unchanged unrelated files. L2 capture/managed CLI fixture
starts from supported setup rather than handwritten store creation; actual-source
metadata rehearsal remains separately scoped. Producer privacy/storage/concurrency
lenses: private bindings contain paths/identities, stdout counts only, no global
cache lock, no existing store replacement and no inference of custody/approval.

## Executed qualification

Three dedicated CLI cases pass: empty-provider readback and no replacement;
capacity/malformed bounds/missing coordination and foreign/missing checkpoint
refusals; two concurrent initializers targeting one output, with one complete
published pair and loser-only cleanup. Unrelated state is preserved. The capture
fixture now bootstraps through setup rather than constructing providers via SDK.
The compiled managed fixture also reuses that journal and shared coordination.

All 57 ordinary native integration cases pass (nine explicit cases ignored),
including capture concurrency, version replay and the new setup cases. Locked
all-target/all-feature warnings-denied Clippy and no-default-feature isolation
pass. The complete real-FFmpeg compiled CLI fixture passes from CLI setup through
capture, planning, managed rendering, retained candidates and explicit Git
proposal/application with source Git offline during jobs (173.33 s).

Actual current private committed scene metadata also passes setup plus capture
in both languages (5.59/5.67 s): 30 primaries, five controls and 1,938-byte private
capture fragments each. Those rehearsals decode/render no primary pictures or
audio and change no production cache or runtime binding. Setup uses isolated
owned temporary providers only. No intended-use, all-family migration, full
recovery or custody acceptance is inferred.

Producer review added filesystem allocation-unit rounding to both capture/setup
metadata allowance, beyond database/rollback and bootstrap budgets. Failed cleanup
keeps conservative accounting; old trusted minimums cannot attest unseen latest
state. All existing global-coverage, compaction/recovery and safe-retirement
obligations remain open.

All three existing owned-preparation unit cases also pass, including preservation
of the conservative capacity record after cleanup failure, explicit stale-record
recovery, unrelated-file retention and overflow/allocation allowance checks.
The final tree passes fmt and diff checks. Hosted Linux/Windows/FFmpeg checks on
the exact published source remain required before merge; this is producer
qualification, not independent or human acceptance.
