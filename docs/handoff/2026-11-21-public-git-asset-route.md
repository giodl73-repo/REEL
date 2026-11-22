# Isolated public git-asset workflow route

P4 input: REEL ee8382b229e7761cfc93080c7f282bfbc8bac23f; public git-asset
3ddeab5572f9479475134e6b7334a26effcb9277 (qualified full hosted run44978592500).
WP-011/016, REQ-001/004/006/007/009/010; existing public native publication/read,
REEL workflow input/handoff contracts. Scope: new standalone crate, preserving
legacy vendor and native route. RUST/GIT/INTEGRATION, synthetic fixtures only.
Producer role checks are not independent or owner acceptance.

`reel-workflow-git-asset` pins the PUBLIC packages through Git, not TRACKER paths
or replacement private vendored names. It captures explicit record/definition
scopes from an immutable commit, resolves selected/exact stream object and logical
identity, previews the named byte closure, and adapts managed guarded reads to
REEL's existing InputReader. No current/latest fallback, enrollment, hydration,
transfer, selection, task completion or scratch deletion follows from construction.
Metadata enumeration/read budgets are explicit; payload access is named only.

This first input port is not a days-long durable job or retained-publication
lifecycle. Persistent Job-class namespace admission, bounded owned preparation,
immutable response ingress, retained reopen-before-completion, cancellation/
restart/uncertain outcome recovery and complete P4 acceptance remain OPEN.
Neither this reader nor a passing metadata preview may be used as a custody
acknowledgement. The old route/SDK and existing jobs remain unchanged.

Entry/exit: compile only the isolated graph, qualify exact historical commit/
stream/identity/budget refusal on synthetic repositories, then add persistent
lifecycle composition and native fixtures before route activation. L0 source/
format/dependency pins/diff; L1 parser/identity/budget negatives; L2 native byte/
job/retained-publication acceptance remains separate. No rendering, provider
calls, cache enrollment, production installation or release is authorized here.

## Input-port checkpoint

Implementation: new isolated crate + lockfile + synthetic-only CI workflow.
Five synthetic integration tests PASS (locked all-target run, 2026-11-21),
covering old/new/uncommitted versions, full commit IDs, missing selection/stream,
exact hash/length, scope escape, invalid descriptors, decoded-preview tampering,
unknown fields and wire/count/byte limits. Strict locked all-target Clippy and
package formatting PASS. These tests execute actual Git metadata reads; they
neither enroll native caches nor qualify durable custody. Hosted results remain
separate and must be pinned to the pushed source head.

Verification: component evidence above. Validation: synthetic metadata behavior
only; native InputReader bytes and full P4 workflow acceptance OPEN. Trace:
WP-011/016 and existing InputReader/source/managed-read ports. Review: producer
checklists below; no independent or human acceptance. No installation or route
activation occurred.

| Producer checklist | Finding and disposition | Remaining gate |
| --- | --- | --- |
| git-asset identity/namespace | Decoded preview must not become source authority. Fixed: Snapshot re-resolves and compares the complete preview before reader construction; forgery fixtures pass. | Native guarded-read acceptance. |
| git-asset storage/custody + concurrency/recovery | A process read guard cannot protect a days-long manual handoff. Explicitly limited in API/docs; durable job and retained publication remain open. | Persistent admission, cancellation/restart and reopen-before-completion fixtures. |
| REEL rights/provenance | Preview includes private IDs/hashes. Classified as local control data, never a public receipt; diagnostics omit supplied bytes/paths. | Returned-result provenance and privacy-safe retained receipts. |
| REEL platform/audience + git adoption | Component pass must not imply a usable production route. Existing native route/jobs preserved; input-only package is experimental. | Owned workspace preparation and complete handoff UX. |

Consume this route through the Rust SDK. Existing CLI metadata review remains
available; this change adds no CLI publication command. REEL still owns prompts,
provider attempts, language timing, triggers, invalidation, findings and closure.
Media type is a validated REEL descriptor, not inferred native-media semantics.
