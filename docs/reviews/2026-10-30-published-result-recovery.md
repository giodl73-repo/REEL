# Verify complete published results before creating a recovery prepare

A retention sink writes immutable result metadata and publishes packet bytes
before recording prepare. An interruption at that boundary leaves the original
input owner intact but no prepare to replay. The separate `retain-published`
command accepts the exact complete packet selection and original operation.
It verifies the original ownership and all packet metadata and cached bytes
under the source fence before it writes either journal transition.
New packet headers pin the retention operation. This command requires that pin
to match; old unbound packets still require the exact recorded prepare receipt.

The existing recorded-only command remains strict. New preparation is explicit
and reported separately, never disguised as an old receipt. Supplied result pins
must also match any already-prepared or completed result on retry. Original input
predecessors and success/failure outcomes remain intact. Read guards span both
transitions, and a partially completed transition can be retried idempotently.

The native fixture exercises success/failure packets both with and without
prepare, invokes the actual commands, and checks wrong owner, unknown fields,
insufficient verification budget, corrupted request and wrong packet selection
leave the no-prepare source and journal unchanged. Repeated adoption adds no
journal events. Later work still refuses.

Producer-applied provenance checklist: this adds generic private custody of an
already-complete packet, not a render proof or approval. No runtime execution,
payload discovery, implicit cache enrollment or removal is introduced. The
profile remains private. This checklist is not independent approval. Recovery
from incomplete/unpublished workspaces, verified scratch retirement, child-owner
handoff and full consumer cutover remain open.
