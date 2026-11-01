# Exact private-review admission

Entry `32f21dd45dbf59726c7286494a080e826ee2c2aa`; native REEL delivery
preparation and managed-job scope, Tig WP-010/011, REQ-003/010. This extends
the existing request and frozen context rather than creating a second renderer,
authority system or cache. L0 fmt/diff/Clippy; L1 exact admission/refusal fixtures;
L2 archived-source replay and managed native render/retention/Git handoff.

An author needs to render a private review before selecting the result. Production
selection must not be fabricated merely to let a draft pass preparation. The
optional request field is a caller-authorized permission for that bounded private
operation, not an approval or permission to distribute it:

```json
"private_review": {
  "checkpoint": "<exact native source checkpoint SHA-256>",
  "records": {
    "delivery/source": {"sha256":"<exact instance SHA-256>","bytes":1234},
    "controls/job": {"sha256":"<exact instance SHA-256>","bytes":5678}
  }
}
```

The record map must contain exactly the otherwise unselected root, controls and
primary inputs actually consumed. Production-selected records continue through
the normal gate and must not be listed. Unused catalog alternatives must not be
listed. Every exception pins the resolved instance, including its definition,
authored fields, status and payload identities, at the exact source checkpoint.
Any revision/checkpoint change needs a deliberately refreshed admission. This
is not a branch-following wildcard or a receipt that can authorize future work.

Only an absent selection_state or the explicit values draft, candidate and
candidate-producer-reviewed are reviewable. Null, malformed, unknown, rejected,
withdrawn and blocked states refuse. An explicit list never overrides those
states or the independently admitted root/control/primary definition boundaries.
Other domain rights and consent checks remain caller obligations; this gate
does not discover them or grant an override of an existing rights restriction.

The map admits 1..4164 entries with bounded IDs and nonempty instance objects
up to 64 KiB each; the serialized admission is itself capped at 64 KiB. Native
request/profile limits also apply. The generated operational list belongs in
private asset-side requests, not a second authored project inventory in Git.

Lowering verifies the checkpoint and every exception's instance pin before
materializing controls. It still resolves every catalog type, checks all actual
consumed slots and receipts and delegates source reads to existing guards. No
media is read merely to grant metadata admission. It refuses extra exceptions
after deriving the consumed scope; metadata/control objects may already have
been read, but no render or source mutation occurs in preparation.

The source descriptor contains rendering parameters and adapter code. Every
lower/freeze/replay still checks the separately supplied exact admission; the
archived source preserves the exact subject records. Managed jobs retain the
complete review request as a bounded input-evidence object through Tig's existing
dependency role, verify it on readback, and preserve it in output handoff roots.
Missing or changed evidence refuses. A source-context candidate alone is not
execution authority. Removing/changing admission cannot turn an archived draft
source into an admitted production context. Original source remains draft;
render/check/retention evidence remains technical candidate evidence. Creative
selection, delivery and release require their separate authorities. There is no
automatic upload or status promotion.

The contained runner has an explicit retention-only evidence route. It keeps
the evidence protected while leaving the ordinary Input role unchanged: the
permission object cannot be submitted as a primary media input. Composition
and claimed-output contexts still refuse on that route. The original runner
entry point still refuses contexts carrying evidence; broader consumption of
Composition/Evidence continues to require the separate existing dependency API.

Permission stays in managed-job evidence, not consumed render parameters. Explicitly
refreshing permission after a metadata-only edit can preserve the desired recipe;
a separate later production selection can too. Old permission still refuses at
the new checkpoint. Equal recipes never substitute for current eligibility or
grant an approval. The production-only parameter encoding remains unchanged.

Omitting the field preserves the production-selected gate and existing request
JSON serialization. Rust request literals now supply private_review: None.
Adapter code identity changes: previously frozen experimental contexts need
their original runtime; do not silently replay them with a different adapter.
No private project admission, installed runtime or production binding is changed
by the generic implementation.

Local qualification passes 20 regular cases, both explicit managed native cases
(production-selected and private review), and locked all-target/all-feature Clippy
with warnings denied. See the [producer review](reviews/2026-11-01-delivery-private-review.md)
for scope and the two integration boundaries found by testing. Hosted gates and
actual project admission remain separate.
