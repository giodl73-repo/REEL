# Durable workflow input claims — producer handoff

The isolated `feat/workflow-handoff` branch now supports explicit input-claim
admission through `reel-workflow-native`. Existing producer SDKs, active project
content and native payload pools are unchanged. Builds, dependency downloads,
synthetic metadata journals and raw evidence remain on F.

The adapter imports the exact EVID-151 slot module and original tests, verified
against the combined qualified source manifest. A small type/fence shim uses
REEL's existing metadata lock and native actor fence domain; the test path shim
does not alter upstream source. The original 247 SDK pins also remain exact.
This source reuse and local qualification are not an installed lifecycle release.

The task/attempt projection contains archived source metadata, canonical task and
input declarations, selected inputs, all declared annex streams and runtime
identities. It uses existing Tig metadata/Tree/Resources/envelope interfaces.
Existing store/controller enrollment, trusted minima and cooperating writers
are required. No setup/bootstrap command or independent cache engine was added.

Explicit acceptance preserves original WorkRef, activation and input selection;
reopen verifies actual complete work, current generation, scope and positive
coverage under the actor fence. A serialized receipt alone is never a guard.
Input claims may protect not-yet-present bytes. Availability, custody, creative
approval, retained-result ownership and task completion are not inferred.

CLI preview changes no claims. Acceptance requires `--apply` and reserves a new
private receipt outside managed roots before effects. It reopens actual native
claims before writing success. Partial files remain; reconcile the original
operation using the same task/attempt/profile and a new destination. Native
pending state blocks a live claims guard; native work remains responsible for
reconciling its original interrupted Prepare. Timeout/cancellation does not
release protection. Close/retire operations are not exposed by this adapter.

Bounds include 16 TOTAL enrolled actors, the existing 128 changed-path phase
limit, conservative envelope overhead, 64 KiB native control nodes, SDK canonical
JSON limits and a 1 GiB declared resource ceiling. Oversized work refuses rather
than splitting transitions or allocating another actor.

Local checks: 68 tests pass: 48 shared contracts/production tests, 19 default
native tests and one explicit Windows guarded read of the existing synthetic
1 MiB payload. Six admission fixtures use real isolated metadata/index journals
with synthetic store declarations and no physical pool. They cover restart/lost
response, original-handle reuse, coverage, archived source restoration, invalid
scope/attempt/generation, CLI partial/no-clobber behavior, budgets and pending
native reconciliation. Four original slot tests retain their existing handoff/
reuse/store-coverage/bounds scope. All SDK/module pins, strict native Clippy,
format and diff checks pass. Hosted CI has not run.

Admission and byte-read fixtures remain separate; there is no complete native
comic production/custody qualification. Persistent fixture roots and exact
source/frozen-tool manifests stay outside Git. An initial evidence filename
collision was caught; original fixtures were intact, and final captures use
separate directories and exclusive writes.

Next: stage exact bounded bytes into an owned resumable handoff while accepted
claims remain live; implement the native handoff lifecycle port; register
external candidates idempotently; protect/publish/reopen and acknowledge retained
ownership before task completion. Expected-baseline selection, retained-owner
actor handoff and guarded scratch retirement remain separate qualification.
No actual film/comic project, source selection or public release was adopted.
