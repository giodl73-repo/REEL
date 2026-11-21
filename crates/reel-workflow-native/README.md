# Protected native workflow handoffs (W3a.2)

This isolated adapter binds exact Git records to explicit annex leaves, admits
positive input claims, stages owned manual work, registers unselected responses,
reopens retained outputs and recovers the original operation. It also connects
W2 film metadata inspection to those native bindings. It calls no model/provider,
executes no film/comic producer, selects no asset and changes no active profile.

It builds on [SDK intake PR #389](https://github.com/giodl73-repo/REEL/pull/389).
The separate workspace/lock preserves the existing producer SDK and root lock.
All original SDK/module source pins remain unchanged. The original slots module
and tests compile from byte-identical build-output copies in their required Rust
module layout; source tests check those copies and the unchanged vendor trees.

## Commands and identity

Run `reel-workflow-native --help` or `capabilities`. The entry points include:

```text
check-snapshot PACKET SNAPSHOT_PROFILE
check-native-inputs PACKET NATIVE_PROFILE
inspect-film PACKET NATIVE_PROFILE INSPECTION_REQUEST NEW_SIDECAR
preview-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT
accept-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE ATTEMPT NEW_RECEIPT --apply
reopen-admission PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT
prepare-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT STAGING_PROFILE NEW_TICKET
export-stage PACKET NATIVE_PROFILE ADMISSION_PROFILE TICKET TICKET_SHA256
create-handoff PACKET HANDOFF_PROFILE NEW_JOURNAL --apply
import-handoff JOURNAL HANDOFF_PROFILE RESULT RETURNS --apply
resume-handoff JOURNAL HANDOFF_PROFILE RETURNS_OR_DASH --apply
preview-close CLOSE_REQUEST REQUEST_SHA256
stage-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
close-retained CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
recover-close CLOSE_REQUEST REQUEST_SHA256 NEW_OUTCOME --apply
```

Profiles are private caller-owned controls, never cache discovery. Snapshot
profiles pin Git, commit, record/definition scopes, every logical record's annex
binding and every explicit stream selection. Pin `declaration_bytes()` as the
task's `native_inputs` artifact. Machine locators are excluded from that authored
identity. Byte-identical alternate leaves do not replace the declared binding.
Read-only checks establish neither durable custody nor production eligibility.

Admission profiles name an already-enrolled effects-enabled Index, metadata
store, coordination root, actor/owner, original operation, store and policy.
They do not create a pool/store, enroll actors or grow limits. Preview observes
headroom; it reserves none. Acceptance preserves the original WorkRef, activation,
source archive and checkpoints. Waiting, timeout or cancellation does not release
claims. Source claims and guarded tool/file handles remain separate authorities.

A handoff profile binds the exact admission receipt, retained target, actual
pinned lifecycle executable and existing acknowledgement directory. The journal
records import intent before native registration. Logical registration identity
uses task/attempt; changed content or provenance conflicts. Uncertain work is
reconciled through its original handles, never a new random operation. Retained
completion requires native work, positive retained coverage and actual reopened
output bytes. Claimed provider provenance is not authenticated and native
retention grants no creative approval, selection or release.

`Returns` names an existing ordinary caller-owned root and a complete artifact
name -> portable relative file map. Returned files are verified without archive
extraction or execution. Owned staging uses exclusive files, preserved capacity
leases and bounded interrupted-copy recovery. Neither source nor scratch is
automatically removed. New private receipt destinations cannot overwrite evidence.

The strict `reel.workflow-native-close.v1` request contains `journal`, `packet`,
`source`, `admission`, `receipt`, `result`, `acknowledgement`, original close
`operation`, `original_lifecycle_tool` and separately pinned closer `runtime`.
Close requires the exact Retained journal and acknowledgement, reopens retained
bytes, and uses the original qualified slot protocol with ordered fences and
coverage checks. It hands coverage to a ready retained actor before clearing a
slot. A new closer does not retarget or rerun an admitted producer. Recovery uses
the SAME request, digest and operation, with a fresh receipt destination. Scratch
retirement, capacity release, payload deletion and retention release are absent.

## Film metadata and bounds

Native inspection supplies the [W2 adapter](../../docs/workflow-film-inspection.md)
only with controls present as exact artifacts in the task's native record/leaf
bindings. Request paths are portable control names in this adapter, not an
annex-leaf search. Project/commit and all artifact pins must agree. Its sidecar
still leaves media, renderer, approval and production adoption unverified.

Historical native v1 profiles retain their original 64 MiB aggregate limit and
unchanged declaration identities. New `reel.workflow-native-snapshot.v2` profiles
must supply a `budget` with schema `reel.workflow-native-input-budget.v1`, positive
`input_bytes` and `input_object_bytes`, up to a declared 64 GiB aggregate. Changing
that budget changes the task-bound declaration. The streaming reader keeps the
input context and native guards alive without a whole-payload buffer; the buffered
metadata reader stays limited to 64 MiB and directs larger reads to streaming.
The qualified single-store SDK and caller's native-reader bounds still apply.

These input descriptions do not expand executable workflow capacity. This slice
retains a 1 GiB protected envelope/retained aggregation scope, 128 MiB response
scope, 512 MiB staging scope, 512 resource objects, 128 changed paths per phase
and 16 total actors. Admission conservatively preflights response/close paths
before effects, counting unknown output hashes separately. Close/retention also
check actual target state. Declared response counts/bytes, the native reader's
per-object allowance and combined input/output envelope with metadata reserve
must fit before acceptance; oversized responses cannot strand a newly started
provider operation at ingestion. Close/retention additionally
check actual target state. The 64 MiB legacy reader bound is not a universal film
limit. Large native metadata can be inspected while an unqualified large job is
refused before admission/production. Controls remain bounded to 1 MiB, with the
SDK's stricter 64 KiB metadata-node bound applying to persisted control nodes.

Composite graph jobs, producer receipts, actual renderer dispatch, large native
episode execution, direct API/proxy calls and new voice takes are not enabled.
Accepted existing film producers remain in use. Further W3a producer/scale work,
W3b providers, W4 review/rebuild and W5 owned adoption/rollback remain required;
this is not Season 5 adoption or full episode-scale qualification.

## Validation

```text
cargo test --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked
cargo clippy --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked -- -D warnings
```

Ordinary tests use synthetic Git metadata, isolated claims/metadata, the original
slot protocol tests, and owned scratch fixtures. Six explicitly ignored Windows
fixtures additionally require a caller-declared existing synthetic pool. They do
not set up or hydrate one. The new close fixture requires a new evidence directory
and preserves its original roots, request and pinned runtime. These explicit tests
exercise native input reads, CLI response replay, retained acknowledgement,
lost completion, staging and close/recovery. They are not real project acceptance.
The qualified SDK/manifest table is retained in [source pins](src/pins.rs).
