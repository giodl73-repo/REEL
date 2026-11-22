# Explicit film issue submission

The Windows owned `reel-workflow-http post REQUEST RAW_SHA256 --apply` command
accepts `payload.kind: github-issue`. It sends the explicitly pinned issue text
through the existing HTTP owner, capacity reservation, intent and response
capture. `resume REQUEST RAW_SHA256` verifies that original captured response
offline. It never issues another POST or needs credentials.

Keep the existing HTTP request's `registry`, `workspace` and `http` sections.
Set the endpoint to exactly `https://api.github.com/repos/OWNER/REPO/issues`,
using the repository's canonical spelling. Set a named authorization secret in
the existing configuration, and keep its response allowance at most 1 MiB.
Replace `payload` with:

```json
{
  "kind": "github-issue",
  "spec": {
    "schema": "reel.workflow-github-issue.v1",
    "repository": "OWNER/REPO",
    "finding_identity": "<identity from the exact exported draft>",
    "draft_key": "<version key from that draft>",
    "issue": { "title": "<exact exported title>", "body": "<exact exported body>" }
  }
}
```

Those placeholders are not runnable values. `KEY-github.json` from
[`draft-film-issues`](workflow-film-findings.md) supplies the `issue` object;
`KEY.json` supplies both identities. Rust callers can use
`github_issue::Spec::from_draft(repository, &draft)` to check the exported draft's
semantic subject/version before composing the request. The adapter does not
re-read the movie or authenticate the reported observation. The pinned request
and explicit `--apply` authorize sending this exact text to that repository.
No media, file bindings or evidence payloads are attached. Issue text contains
project identities and hashes; keep it in an authorized destination.

The request is bounded to a 256-byte title and 32 KiB body, including exactly
one final finding marker and its draft-version line. The adapter pins GitHub API
version `2026-03-10`, JSON Accept and User-Agent headers. Only the official GitHub
endpoint is supported for service execution. Explicit loopback HTTP remains a
synthetic fixture path, whose receipt sets `simulated: true` and
`issue_submitted: false`. Enterprise/proxy issue services need their own contract.
The API shape and created response follow the
[official GitHub issue contract](https://docs.github.com/en/rest/issues/issues?apiVersion=2026-03-10#create-an-issue).

## Identity and recovery

The operation ID binds the case-normalized repository and logical finding
identity, independently of its note version. Reuse the **original registry**.
A changed note, request configuration or workspace cannot allocate another
operation for that finding in it. An existing remote finding needs an explicit
update workflow; this adapter never silently opens a replacement issue.
GitHub does not treat REEL's marker or operation header as a service idempotency
key. Using another registry or losing the original evidence defeats the local
fence and is not recovery. Pre-existing issues created outside this registry
must be reconciled before invoking this command.

A complete byte-verified HTTP 201 response must match the exact title, body,
positive issue number and official repository issue URLs, and must not be a PR.
The owned directory then receives an immutable `issue.json` receipt binding the
original operation, finding/version and response hash. Offline resume verifies
the same response and receipt. A changed/partial existing receipt refuses and
is preserved. Terminal status omits issue content and repository identity.

Every other response, malformed or conflicting 201, timeout, lost completion
record or truncated response preserves original evidence for diagnosis. No
automatic retry occurs, including for Busy/Denied/Conflict classes. Missing or
uncertain service outcomes require reconciling the **original** issue operation
with the service; this release does not provide an authenticated remote GET or
manual uncertain-outcome adoption command. Do not start under a fresh identity.

An issue receipt establishes no viewing, observation accuracy, rights authority,
native retention, workflow task completion, repair, selection or publication.
Scratch/capacity and native claims retain their existing separate lifecycle.

## Qualification

Portable contract tests cover identity/version separation, endpoint refusal,
exported-draft binding and exact created-response checks. Windows synthetic
loopback tests exercise the real owned runner, service headers, offline replay,
changed-note refusal, altered-receipt preservation, complete non-created replies
and interrupted creation. Existing raw/proxy/transport tests also run. No live
account, production finding, profile, movie, recording or approval is changed.
