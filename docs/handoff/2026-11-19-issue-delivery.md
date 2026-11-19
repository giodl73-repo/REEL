# Explicit issue delivery and reconciliation

The shared film/comic review model now has an explicit issue-delivery adapter.
`reel-workflow::tracker::draft` binds one checked finding, its report hash and
repository route to caller-authored title/body. It does not copy private review
prose, source descriptors or evidence into the remote payload. Generic REEL/Tig
routes require a declared synthetic reproduction descriptor; that declaration
is not proof of rights, reproduction quality or approval.

```text
reel-workflow-native export-issue-draft PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK ROUTES ISSUE_TEXT NEW_DRAFT --apply
reel-workflow-http submit-issue TRACKER_CONFIG DRAFT --apply
reel-workflow-http reconcile-issue TRACKER_CONFIG DRAFT QUERY_ID --apply
```

The first command reopens the retained report under native proof and exclusively
writes a private draft. The other commands perform external effects only when
explicitly invoked. They implement a custom proxy protocol, not GitHub's API.
No real tracker was contacted during qualification.

## Proxy contract

Tracker configuration uses schema `reel.workflow-tracker.v1`, an existing
absolute private `registry` directory, `create` and `lookup` HTTP transport
configurations, and a `repositories` map from lowercase `owner/repo` to an
explicit canonical HTTPS issue-URL prefix ending in `/`. HTTP configuration
retains the existing credential-reference, endpoint and request/response bounds.
The complete configuration is pinned into each local call intent.

Create JSON contains only `schema: reel.workflow-issue-create.v1`,
`operation_id`, `repository`, `title` and `body`. The operation ID derives from
repository and stable finding key. The proxy MUST atomically enforce uniqueness
of operation ID and exact payload, rejecting a different payload under that ID.
Local locking alone cannot provide cross-machine remote uniqueness.

Lookup JSON contains `schema: reel.workflow-issue-lookup.v1`, the original create
`operation_id`, `repository` and original create `request_sha256`. Its transport
operation ID derives separately from the create ID and explicit query ID.
Lookup MUST be noncreating. A not-found result never permits another create.

Both successful endpoints return a JSON receipt with
`schema: reel.workflow-issue-receipt.v1`, original create `operation_id`,
`repository`, original create `request_sha256`, `issue_id` and `issue_url`.
The URL must equal the configured repository prefix plus the returned ASCII
token ID. Responses from another repository or with mismatched pins refuse.

## Durable local boundary

The registry preserves exact request bytes, intent, response headers, received
body prefix, completion marker and checked receipt under deterministic names.
Intent is flushed before network execution. Same-operation ownership is
exclusive. Once create intent exists, submit never sends another create,
including after HTTP errors, timeout or truncated responses. Explicit lookup
can recover a remote issue after a lost reply. Replaying a receipt verifies its
original local request and completed response. A complete original response can
also recover a missing local receipt without another POST.

Keep the original registry, draft and configuration on uncertainty. Do not clear
intent, switch registries or allocate a replacement operation to bypass refusal.
Partial control writes and corrupt evidence require diagnosis; they are preserved.
New calls have an observed 4,096-entry admission guard, not a concurrent global
allocator or a filesystem quota. Each control/response is bounded to 1 MiB;
HTTP duration is at most 300 seconds. Existing receipt reads remain available
when new-call admission is full. Qualification is Windows/NTFS and loopback.

This registry is private transport evidence, not a second asset/cache engine.
It does not establish native retained ownership, complete a workflow task, apply
a repair, select an asset or close an issue. Those boundaries remain separate.
Private title/body text must be reviewed for the intended tracker audience.

## Producer checklist review

Applied the repository rights/provenance and platform/audience checklists to the
delivery boundary. Explicit text and repository scope prevent automatic copying
of private findings into a tracker; local controls remain private. The adapter
cannot determine whether authored text is safe for its audience. No creative,
translation, rights or release approval follows from a technical receipt.

Qualification evidence and remaining integration work are recorded separately;
actual service behavior, cross-machine proxy uniqueness and fix/closure execution
remain unqualified.

## Qualification

Source commit `0b161cf04191196de9606c47b8d5123f17eddd01` passed the 87
workflow tests, 18 HTTP tests and 58 standard native tests, plus strict Clippy
for the workflow/HTTP crates and native builds with and without film support.
All 247 vendored SDK source pins and all 1,889 archived source files verified.
The five tracker tests include CLI submission/reconciliation, no second create
after a lost reply, original-response recovery, URL binding and ownership refusal.

The explicit native fixture passed in 120.97 test seconds (121.78 wall seconds),
reopening the existing retained synthetic comic-review report. It exported only
explicit text, refused a mismatched repository before creating an output and
left claims unchanged. Its preserved root is
`F:/src/tig-dev-temp/.tmpiEvubc`; the original retained report remains at
`F:/src/tig-dev-temp/.tmpYT3pCe`.

The frozen tool/source/log manifest is
`F:/src/tig-dev-temp/reel-issue-delivery-20261119-v1/qualified-tools.json`,
SHA256 `3d9b1002b53ed9a373c0fe64ed4e2af65e656182fcd11fae2f21287d55dd4713`.
Standard full-suite results were observed in tool output; the evidence directory
contains the final tracker and selected build/lint logs, check summary and actual
native fixture log. This qualification does not claim an end-to-end live tracker
or a native-retained delivery task.
