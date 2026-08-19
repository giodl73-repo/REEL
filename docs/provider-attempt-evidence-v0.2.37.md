# Provider-attempt evidence and resume planning (v0.2.37)

REEL v0.2.37 accepts sanitized provider-attempt evidence produced by an
owner-controlled adapter. REEL does not submit, poll, download from, retry, or
otherwise call a provider. Provider credentials, prompts, raw payloads, URLs,
private errors, and local paths remain outside every portable output.

## Immutable receipt

`provider-attempt-receipt` consumes
`reel.provider-attempt-input.v0.1` and writes a no-clobber
`reel.provider-attempt-receipt.v0.1`:

```powershell
cargo run -- provider-attempt-receipt attempt-input.json `
  --output-path attempt-receipt.json --output json
```

The strict input binds:

- stable intent and attempt IDs plus a one-based attempt sequence;
- production-manifest, generation-plan, requested-policy, and resolved-
  configuration SHA-256 values;
- `initial`, `retry`, `retake`, `remix`, or `extension` operation kind;
- shot, cue, and/or exact millisecond span scope;
- a portable provider identifier and SHA-256 of the opaque provider job ID;
- canonical UTC lifecycle observations;
- an honest replay grade;
- an exact parent receipt for every non-initial operation.

Lifecycle observations must form one of these immutable paths:

```text
submitted
submitted -> running
submitted -> completed
submitted -> failed
submitted -> running -> completed
submitted -> running -> failed
```

`completed` may remain pending capture or include one verified PNG. `failed`
requires a normalized failure classification. Submitted and running attempts
cannot contain artifact or failure evidence. A retry requires a failed parent;
a retake, remix, or extension requires a completed parent.

For captured output, REEL reads the local PNG once and uses those same bytes for
SHA-256, byte count, PNG detection, and decoded dimensions. Captured output uses
`exact-byte-reuse`; pending completed output may use
`deterministic-local-replay` or `best-effort-provider-replay`. Non-completed
attempts use `best-effort-provider-replay`.

## Independent checking

`provider-attempt-check` validates the portable receipt independently. Supply
`--artifact-path` only for a captured completion:

```powershell
cargo run -- provider-attempt-check attempt-receipt.json `
  --artifact-path captured.png --output-path check.json --output json
```

The checker rejects unknown fields, invalid authority flags, malformed
operation/parent shape, missing captured artifacts, extra artifacts for
non-captured attempts, and hash, byte-count, PNG media-type, or dimension
mismatches.

## Deterministic resume planning

`provider-attempt-resume` consumes
`reel.provider-attempt-resume-input.v0.1`. The input supplies the current intent
and four current contract hashes plus local receipt files pinned by SHA-256:

```powershell
cargo run -- provider-attempt-resume resume-input.json `
  --output-path resume-plan.json --output json
```

REEL hashes and parses each receipt from the same bytes, validates every receipt
before considering sequence order, rejects duplicate attempt identities or
sequences, and requires a complete sequence-one parent chain. A captured latest
attempt must also provide a hash-pinned `captured_artifact` binding before REEL
will choose reuse; missing current bytes return `capture-output`, and changed
bytes fail verification. It then emits exactly one path-free decision:

- `reuse-captured`
- `capture-output`
- `poll-existing`
- `retry-terminal`
- `blocked-stale-input`

Each decision includes a stable reason code. The planner never contacts a
provider and never selects, promotes, or supersedes an output.

## Human authority boundary

Receipts, check reports, and resume plans state:

```text
provider_executed_by_reel = false
human_authority_required = true
```

Creative, rights, publication, and release approval fields remain false.
Technical capture, verification, replayability, or completion does not select
an output or grant any approval.
