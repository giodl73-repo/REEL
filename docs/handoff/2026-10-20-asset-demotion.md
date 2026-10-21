# Guarded asset demotion checkpoint

Added an explicit Windows-only redundant hot-copy removal command in reel-assets.
It verifies both copies, pins the retained file, checks all declared consumer
protections, refuses live/unreconciled admissions, holds control files against
changes, deletes exact opened source links, and journals intent/completion/recovery.
The job runner now reserves before resolving source paths. See asset-demotion-v1.

Local validation: 73 asset-crate tests (including 18 new cases), Clippy with
warnings denied, release build, formatting, role structure and whitespace.
The prior pulse's full root suite and Windows/Linux/FFmpeg CI also completed
successfully. This slice's new CI is a separate gate. Public fixtures remain
synthetic. Private command runs and source/executable hashes remain upstream.

Producer-applied rights/provenance review: no private source/media, consumer
names, hashes, real cache paths or reports imported into public code/tests.
Technical verification does not select creative assets or broaden rights.

Actual production apply remains held: live/unreconciled reservations and an
unsupported legacy consumer protection contract must be reconciled without
stopping foreign jobs or editing their worktrees. Cold custody, independent
LFS copies, last-copy retirement and non-Windows destructive application remain
outside this slice. No production-space reclamation is claimed here.
