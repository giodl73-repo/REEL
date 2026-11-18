# Resumable workflow input staging — producer handoff

The isolated REEL workflow branch now prepares owned private workspaces and
exports verified native inputs under original accepted claims. It adds two CLI
entries: `prepare-stage` writes a new private ticket; `export-stage` requires that
ticket's external raw SHA256 pin. Runtime/record/attempt identity remains fixed.
No provider is invoked and no output candidate is imported or selected.

Preparation requires existing scratch/coordination roots, actual native claim
readback and an explicit capacity policy. It reuses the REEL reservation ledger
and OS namespace identity machinery. The ticket pins directory identities and
intent, preserves original native handles and names the preserved capacity entry.
Resume reacquires that exact lease; it never allocates another reservation.
All original SDK/module source pins remain unchanged.

Every export verifies actual claims, ownership, controls and original native
payloads. Partial copies use bounded exclusive immutable parts; a flushed part
can be reused after interruption, while an incomplete part remains and another
part is created. Completion follows verified files. Directory replacement,
tampering, extra files, wrong ticket pins, absent native providers and exhausted
bounds stop without repair, cleanup or release. A valid scratch copy cannot
replace a missing canonical provider. Generic bindings and prompts are portable;
comic producer-relative source files are preserved and source correspondence is
checked before writes. No comic render or creative approval is inferred.

Stage limits: 512 MiB maximum logical workspace; 256 exported files; 2,048 total
entries; 16 path segments; four part attempts per file. Parts and hard-linked
targets both count toward logical usage. Existing input/control/native phase
bounds remain. Runtime free-space policy and original shared capacity admission
are preserved. Neither waiting, completion nor failure releases capacity or
input claims. Retained-result verification and guarded retirement remain separate.

Qualification: 107 targeted tests pass — 48 shared workflow/publication/print,
32 shared capacity/job regressions, 25 default native tests and two explicitly
invoked Windows native fixtures. Five filesystem staging faults test restart,
partial/flushed copies, evidence preservation, tampering, identity and bounds.
The CLI preflight fixture leaves only intent when native bytes are unavailable.
The combined fixture uses an existing synthetic 1 MiB payload, real isolated
native metadata claims, guarded provider reads, preserved capacity and separate
CLI processes. Its private cooperating-controller enrollment is an explicit test
assumption. No native payload pool is initialized or published to; no provider
default, active project profile or selection changes. Strict Clippy, both fmt
checks and diff checks pass locally; hosted CI has not run.

Persistent synthetic fixtures, source manifests and frozen tools stay outside
Git. Existing tool/source profiles are retained. The Windows/NTFS fixture does
not establish universal storage behavior, hostile local control-file protection,
production pool adoption, creator rights or full film/comic readiness.

Preparation failure before a complete ticket can leave a directory, partial
ticket and conservative capacity record requiring explicit diagnosis. This
boundary is preserved, not automatically adopted or retried under a fresh job.
Power-loss directory-entry durability is not qualified. Active calls hold
namespace/input guards; long waits rely on durable claims and retained controls.

Next: register external responses idempotently by original task/attempt/artifact;
protect/publish/reopen and acknowledge retained ownership before completion;
connect the native result lifecycle to the existing handoff coordinator. Expected
baseline selection, retained-owner actor close and guarded scratch retirement
remain separate. No production project or public release was adopted.
