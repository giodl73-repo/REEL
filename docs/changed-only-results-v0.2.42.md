# Immutable changed-only results (v0.2.42)

REEL v0.2.42 records exact owner-produced results for v0.2.41 changed-only
plans. REEL regenerates the plan from current graph and prior-state evidence,
verifies every expected output byte, emits a path-free immutable receipt, and
advances local state through a separate no-clobber command.

REEL does not run the operation. The owner system remains authoritative for
execution, environment, success semantics, output choice, and whether a result
should advance its local state.

## Workflow

After `changed-only-plan` reports a node as `rebuild`, the owner executes its
existing script or tool and writes a small local result binding:

```json
{
  "schema": "reel.changed-only-result-input.v0.1",
  "graph_id": "bertica-s1e01",
  "node_id": "measured-visual-conform",
  "action_key": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "owner_result_id_sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
  "outcome": "completed",
  "outputs": [
    {
      "file_id": "production-manifest",
      "path": "production/reel/S1E01.yaml"
    }
  ]
}
```

The action key is the execution handshake returned from the exact plan. The
owner result reference must already be SHA-256 sanitized. Output bindings carry
only identities and local paths; REEL measures hashes and byte counts.
Relative output paths resolve against the result-input file, not the caller's
working directory.

Create the portable receipt:

```powershell
cargo run --bin reel -- changed-only-result-receipt `
  graph.json state-v1.json plan.json result.json `
  --output-path result-receipt.json
```

Then advance local state:

```powershell
cargo run --bin reel -- changed-only-state-advance `
  graph.json state-v1.json plan.json result.json result-receipt.json `
  --output-path state-v2.json
```

The commands are deliberately separate. A receipt remains valid if state
publication fails and can be retried. Neither command overwrites an existing
file.

## Receipt-generation checks

`changed-only-result-receipt`:

1. loads the exact graph and prior state;
2. re-runs changed-only planning against all current recipe, direct-input,
   dependency-output, and prior-output bytes;
3. serializes the regenerated plan canonically and requires byte-for-byte
   equality with the supplied plan;
4. requires the named node to be `rebuild` with the owner-returned action key;
5. requires every expected output identity exactly once;
6. resolves each output to a distinct regular file and measures its SHA-256 and
   byte count;
7. writes one path-free `reel.changed-only-result-receipt.v0.1`.

`blocked-dependency` and `exact-byte-reuse` nodes cannot create new result
receipts. Missing, extra, duplicate, directory, or physically aliased outputs
are rejected.

## Portable receipt

The receipt records:

- exact graph, prior-state, and plan SHA-256 values;
- graph, node, operation, and action identity;
- the sanitized owner result reference and owner-attested `completed` outcome;
- sorted output identities, SHA-256 values, and byte counts;
- whether the plan was regenerated and output bytes were verified;
- explicit execution, state, selection, approval, publication, and release
  boundaries.

It contains no local paths, commands, stdout/stderr, source text, prompts,
provider payloads, credentials, or private job identifiers. It is deterministic
for the same exact evidence and does not claim a timestamp, trusted builder,
signature, SLSA provenance, or in-toto attestation.

## State advancement

`changed-only-state-advance`:

- regenerates the exact expected receipt again from the graph, prior state,
  plan, result binding, and current output bytes, then byte-compares the
  supplied receipt;
- requires the exact prior-state bytes named by the receipt;
- requires result identity, action key, sanitized owner reference, and outcome
  to match;
- re-reads every current output and compares its identity, SHA-256, and byte
  count with the immutable receipt;
- replaces only that node in a sorted local state snapshot;
- binds the receipt SHA-256 on the node;
- emits `reel.changed-only-state.v0.2`.

State v0.2 is path-rich local operational input, not a shareable receipt.
New output paths are canonicalized before persistence, and retained relative
state paths are rebased against their owning state file, so future checks do
not depend on a caller's working directory. Distinct output identities may not
resolve to the same physical file, including through hardlinks.
Changed-only planning accepts both state v0.1 and v0.2. Migrated v0.1 nodes may
remain without receipt bindings; every node advanced through v0.2.42 carries
one.

Exact prior-state binding allows explicit branches: two different completed
results may each advance the same prior snapshot into separate new state files.
REEL does not choose between them. Automatic merging, promotion, rollback, or
history traversal is deferred.

## Authority boundaries

An owner-attested completed outcome means only that the owner adapter reports
external execution completion. REEL independently verifies plan and output
bytes but does not prove the runtime environment, hermeticity, determinism,
creative quality, canon fidelity, rights, spending authority, publication, or
release readiness.
