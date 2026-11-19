# Tracker close and original-operation reconciliation

The tracker HTTP adapter now supports an explicit close effect and read-only
lookup reconciliation. It shares the original create adapter's guarded registry,
per-operation lock, exact request/config/draft bindings, intent-before-POST,
response-head/prefix preservation and complete-response validation. Existing
create payloads, call/response/delivery schemas and receipt types remain v1.

```text
reel-workflow-http close-issue CLOSE_CONFIG ORIGINAL_DRAFT CLOSE_DRAFT --apply
reel-workflow-http reconcile-issue-close CLOSE_CONFIG ORIGINAL_DRAFT CLOSE_DRAFT QUERY_ID --apply
```

`CloseConfig` uses `reel.workflow-tracker-close.v1`, with two existing tracker
configuration objects:

- `issue_delivery`: the original create registry, endpoint/config and allowed
  repository URL prefixes. Close verifies the complete stored original delivery
  locally; it never sends another create. A serialized receipt alone is insufficient.
- `transport`: the close registry/config. Its existing `create` transport field
  names the close-effect endpoint; its `lookup` field names the strictly
  noncreating close-receipt lookup endpoint. Reusing this transport object keeps
  one durable execution implementation rather than a second storage engine.

Both registries must already exist as ordinary owned private directories. The
existing secret-header references, HTTPS policy (or explicit synthetic literal
loopback HTTP), byte limits and bounded timeouts apply. Original create evidence
must remain accessible; receipt-only historical adoption is still separate work.

## Close intent

`reel_workflow::tracker::close::draft` binds an original issue draft, its exact
delivered target, a checked resolution proposal and explicit close text. The
`reel.workflow-issue-close-text.v1` control names `finding_key`, `expected_revision`
and `note`. A missing finding, different baseline report, wrong create payload
hash or mismatched target refuses. The original finding's create operation and
historical URL remain intact. The resolution proposal constructor is a pure
contract check, not authenticated delivery or native custody.

The private close draft pins repository, finding/baseline, original create
operation/request, issue ID/URL, resolution digest, expected revision and note.
Only explicit note text and the necessary operation/target/revision/resolution
fields enter the remote `reel.workflow-issue-close.v1` payload. Private report
text, source descriptors, finding bodies and baseline hashes are not copied into
the payload. Changes to note or expected revision retain the same operation ID
for that issue/resolution, and conflict with any consumed original specification.

## Proxy transaction contract

The proxy must atomically compare `expected_state: open` and `expected_revision`,
close that exact issue, and record operation-ID/payload uniqueness together with
the resulting receipt. A successful `reel.workflow-issue-close-receipt.v1` must
bind original close operation, repository, request hash, exact issue ID/URL,
resolution digest, `state: closed`, expected previous revision and a distinct new
revision. The client validates every field and the configured URL prefix.

Lookup uses `reel.workflow-issue-close-lookup.v1` and must return the receipt of
that original close operation, not infer success from an issue's current closed
state. A later reopen does not erase historical delivery. A fresh query ID permits
another explicit lookup only. Not-found, HTTP failure, truncation or invalid
receipt never permits another close POST. Conflict/replanning must preserve the
original operation; there is no automatic reset or force-close fallback.

## Durable target fence

Close owns a separate lock/immutable owner record for the original issue target,
in addition to the shared per-operation transport lock. Another resolution,
changed note, changed revision or changed configuration cannot silently replace
an uncertain close. The consumed-close fence is durably written inside the
transport's intent callback before HTTP execution. It prevents another effect
when per-call files are unavailable, while preserving received response prefixes
and correlation headers for reconciliation.

A surviving target lock without its owner record refuses reconstruction. Corrupt
or partial records require recovery; no bytes are deleted to manufacture a new
attempt. Target protection has no expiry. Explicit reopened-issue generations
and owner handoff are not implemented by this first-close contract. Complete
loss of all protective registry evidence cannot be repaired by guessing; retain
the original controller identity/evidence and reconcile it. The directory-entry
guard is an observed admission bound, not a filesystem quota.

## Authority and qualification boundary

This is a transport adapter. It verifies original create delivery and exact close
request/response bindings, but the caller still owns admitted native inputs,
retained resolution adoption, rights and selection policy. The native workflow
must revalidate/adopt its resolution evidence before invoking this effect; a
hand-authored deserialized close draft does not grant native authority. These
native delivery/close retention and scheduler integrations remain open.

The protocol is a custom proxy contract, not a GitHub API implementation or real
tracker qualification. Tests call only a local synthetic server. No actual issue
is closed, no public message is sent and no creative/release approval is implied.

Producer-applied rights/provenance review preserves explicit egress text,
historical issue identity, immutable original evidence and technical/creative
separation. No independent review is claimed.
