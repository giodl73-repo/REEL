# Draft records for authored control edits

```text
reel-tig-register --control-edit PRIVATE_PROFILE NEW_RECORD.tig
```

This worker constructs an unreviewed record after an editor has produced a new
JSON control. It preserves the exact asset ID, definition, authored fields,
ordered streams and metadata outside its reserved lineage key. The selected
Object changes; optional selected-pin ID/media-type annotations remain intact.
It does not read old media, open Git, publish to a cache or adopt a source.

Use `reel.tig-control-edit-candidate.experimental-1` with:

| Field | Meaning |
| --- | --- |
| `asset_id` | Exact independently expected target logical ID |
| `original_record` | FileRef to the exact predecessor `.tig` bytes |
| `definition` | Existing `definition` and `authored_schema` FileRefs |
| `expected_selected` | Exact predecessor Object |
| `replacement` | FileRef to the newly authored JSON control |
| `edit_request` | FileRef to the exact JSON editor request |
| `source_checkpoint` | Caller-declared source identity; not ownership proof |
| `max_control_bytes` | Positive control allowance, at most 4 MiB |
| `payload_filename` | Optional new sibling `.json` output for managed retention |

FileRefs carry path, SHA-256 and length. Paths are explicit absolute paths or
portable paths relative to the profile. Records, definitions, schemas, requests
and profiles remain bounded to 64 KiB. Native control decimals are preserved as
raw bytes; parsing a JSON object is not native semantic or schema qualification.
Windows readers exclude writes/deletion while constructing/publishing candidates.
On other platforms this remains a trusted, caller-owned input namespace.

Current status becomes `draft-unreviewed`, with no human approval or production
claim. The complete predecessor status is retained under
`metadata.reel_control_edit.predecessor_status`; original rights and authority
metadata stay intact, referring to their original subjects. This is predecessor
evidence, not renewed policy admission for the new subject. The caller must
reapply project eligibility before production. Lineage also pins the predecessor
record/selection, exact edit request, candidate profile and declared checkpoint.
Repeated edits refer to their predecessor record rather than embedding an
expanding copy of all prior revisions. Unknown preexisting reserved lineage
schemas refuse. Nothing interprets a project-specific approval vocabulary.

Optional `payload_filename` copies only the pinned authored JSON control to a
new sibling file. Both destinations must be unused; reserved/escaping names and
record/control collisions refuse. Each file is synced and published without
replacement. This is not a two-file transaction: a late failure can leave an
owned partial candidate set for the managed runner's failed-job retention.
Source files and existing destinations are never overwritten or cleaned up.

## Managed handoff

The [frozen-job CLI](tig-frozen-job.md) replays an already enrolled generic job
through this existing runner without adding setup or adoption authority.

Use the existing job runner to retain **both** control and record as declared
outputs. The inline request and installed worker must belong to the independently
frozen/reopened job context; protect its exact source owner/revision before
execution. The runner's ordinary capacity admission, source fence, verified CAS
publication and output-root readback remain authoritative. No global cache lock
or new provider is introduced.

Only after successful retention/readback should the caller load those exact
record bytes into the existing Tig Git-proposal API, with the exact old path and
raw-byte expectation. A proposal creates an isolated ref, not a checkout or merge.
The durable job output root must stay protected until the consumer's source
adoption or explicit handoff/retirement protocol supersedes it. Git creation by
itself releases no byte obligation. Generic job success remains unproven media
production; new creative approval, complete closure and custody are not inferred.

The worker deliberately reports `outputs_retained: false` and
`source_ownership_verified: false`: a bare invocation cannot make those claims.
Its fixed public summary/errors omit private paths, IDs, hashes and text.

## Verification scope

The CLI fixtures cover exact predecessor/definition/payload/request pins,
stable identity, original authoring/history/rights/authority preservation,
unreviewed status, decimal controls, no-clobber and malformed/budget/path refusals.
The Windows managed fixture freezes and reopens a real Git source/job context,
rejects a foreign owner without mutation, executes this actual CLI, retains both
outputs, reopens the source journal and verifies their exact output-root pins,
then creates a one-record Git proposal while HEAD and scratch remain unchanged.
All bytes, repositories and stores in that fixture are synthetic.

Actual project replay, native control semantics, browser review and consumer
adoption are separate checks. This worker is one component of that workflow;
it is not an authoring-completion or production-readiness assertion.
