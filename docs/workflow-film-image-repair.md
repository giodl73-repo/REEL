# Prepare a film image repair

`reel-workflow prepare-film-repair REQUEST RAW_SHA256 NEW_DIRECTORY` resolves a
reported picture finding against its original cel-ID movie/map/receipt, checks
the declared original renderer job and source bindings, and exports an ordinary
image-candidate task. It reads only explicitly bound files. It performs no
generation, API call, issue action, selection or consumer rendering.

The output contains `prepared.json`, a one-task `plan.json`, and `work` with the
existing packet, private prompt and response contract. Use existing
`capture-work`/`accept-work` for manual returns. Direct API/proxy callers use the
same packet/result contract. Native callers must still supply a qualified exact
snapshot declaration, protect inputs, retain/reopen the returned candidate and
reconcile the original operation through the landed lifecycle interfaces.

## Pinned request

The request's schema is `reel.workflow-film-image-repair-export.v1`. It contains
`baseline` (the complete declared consumer Plan), `report` (the original findings
report), `repair` (below), `attempt_id`, and explicit logical-ID-to-file `files`.
Pin its raw SHA externally. No repository, cache or directory discovery occurs.

`repair` has schema `reel.workflow-film-image-repair.v1`:

| Field | Purpose |
| --- | --- |
| `reviewed_task` | Exact original `FilmRender` task ID in the baseline plan |
| `finding_key` | Immutable version key from the original checked report |
| `task_id` | New image-candidate task ID, distinct from baseline tasks |
| `instructions` | Authored correction/continuity/style instructions, at most 16 KiB |
| `reference_inputs` | Up to 16 explicitly named additional inputs from that original renderer task |
| `native_inputs` | Optional caller-authored exact declaration for the new input subset; never copied implicitly from the old full rendering closure |
| `output` | One required PNG, JPEG or WebP `OutputContract`, returned as `picture` |
| `input_bytes`, `object_bytes` | Explicit candidate-input sum/object allowances; positive, object at most sum, sum at most 64 GiB |

The selected original image is always an input. Its producer-relative path must
resolve through the declared original task's exact `source_bindings`; its logical
artifact, hash and length must agree with that reviewed target. Finding project,
language and source commit must match the reviewed task's exact snapshot, including
scoped historical task snapshots. The review map's original job must match the
tool's `publication` and its declared path binding, not the outer task request.
No matching-byte search or latest drawing substitution is used.

The report is checked again against the original movie/map/receipt bytes. Its
reported text, C/story/event identity and frame interval remain in the private
prompt. Only explicit original image/reference payloads become candidate task
inputs; the movie, recordings and unrelated renderer resources are not included
automatically. A provider must separately qualify support/egress for those image
formats and references. Returning a PNG is not proof of visual correctness.

This is a metadata/byte binding to the **caller-declared baseline**. It does not
authenticate that baseline as the original retained render invocation, resolve
record-to-annex custody or verify Git provenance. A native production integration
must bind the original retained packet/worker evidence and source snapshot before
admitting the repair. Do not use a detached issue body or this package as a
selection/adoption capability.

## Consumer impact and later application

Impact lists exact-pin direct consumers, their dependency closure in topological
order, unaffected tasks and affected languages. A shared selected drawing can
affect both ES and EN; a different logical record with identical bytes is not
silently substituted. Historical versions remain distinct. The complete declared
baseline graph is the scope: consumers omitted by the caller cannot be discovered
or certified by this command.

Only the new candidate task is dispatchable in its exported plan. The old consumer
plan is unchanged. The impact is a required rebuild list, not a plan that silently
reuses stale precomputed controls or parent outputs. After candidate review,
explicit baseline-checked selection/control edits and observed parent-result
bindings must produce the corrected consumers. Their execution, exact approval
decisions, native retention and owned adoption remain separate open gates.
Old approval subjects are not transferred to the candidate. Existing and partial
export destinations refuse and remain untouched; reuse the original generated
work/attempt rather than creating a new provider effect on uncertainty.

## Runnable synthetic example

```text
cargo run --example film-review-workflow --locked -- NEW_ABSOLUTE_DIRECTORY --apply
```

The existing color/silence walkthrough now additionally prepares a controlled
C101 image repair, captures/replays a generated green PNG through the existing
manual result path, and renders a separate candidate review. It checks that the
center changed from blue to green while C/story identity and actual frame interval
remain the same. It compares D/M/E/mix WAV bytes, the untouched target and original
job, and shares that media path with the root regression test. Original job/media
stay in place. The receipt is synthetic:
there is no live provider, authenticated original native task, native retention,
selection, transferred approval or bilingual rebuild qualification.
