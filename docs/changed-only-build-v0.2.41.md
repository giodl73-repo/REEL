# Changed-only build planning (v0.2.41)

REEL v0.2.41 validates an owner-authored dependency graph and emits a
deterministic, path-free changed-only plan. It verifies exact local recipe,
input, and reusable output bytes, but it never executes a build, discovers
implicit dependencies, mutates a cache, selects creative output, or grants
approval.

This is a local planner, not a render farm or remote cache protocol.

## Command

```powershell
cargo run --bin reel -- changed-only-plan graph.json state.json `
  --output-path plan.json
```

`graph.json` and `state.json` are private local inputs and may contain local
paths. `plan.json` contains only portable identifiers, hashes, byte counts,
statuses, reasons, source-document hashes, and explicit authority boundaries.
Publication is atomic and no-clobber.

## Graph contract

The strict `reel.changed-only-graph.v0.1` schema contains:

```json
{
  "schema": "reel.changed-only-graph.v0.1",
  "graph_id": "bertica-s1e01",
  "nodes": [
    {
      "node_id": "shot-register",
      "operation_kind": "contract-build",
      "recipe": {
        "file_id": "register-script",
        "path": "tools/build_register.py",
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "bytes": 1234
      },
      "inputs": [
        {
          "file_id": "source-contract",
          "path": "production/source.yaml",
          "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
          "bytes": 5678
        }
      ],
      "dependencies": [],
      "expected_outputs": ["register"]
    }
  ]
}
```

Every recipe and direct input declaration is verified against current local
bytes. IDs and operation kinds are bounded portable tokens. Nodes reject
duplicate identities, duplicate edges, self-dependencies, unknown
dependencies, and cycles. At least one expected output identity is required.
REEL does not infer undeclared files read by an owner script.

## Prior-state contract

The strict `reel.changed-only-state.v0.1` schema records owner-produced local
outputs from an earlier action:

```json
{
  "schema": "reel.changed-only-state.v0.1",
  "graph_id": "bertica-s1e01",
  "nodes": [
    {
      "node_id": "shot-register",
      "action_key": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "outputs": [
        {
          "file_id": "register",
          "path": "production/register.json",
          "sha256": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
          "bytes": 9012
        }
      ]
    }
  ]
}
```

The owner-controlled adapter updates this state only after its build succeeds
and it has measured the new output bytes. REEL does not write state on behalf
of an executor.

## Action keys

Each action key is SHA-256 over canonical JSON containing:

- the action-key schema, graph ID, node ID, and operation kind;
- exact recipe identity, hash, and byte count;
- exact sorted direct-input identities, hashes, and byte counts;
- exact sorted expected-output identities;
- exact sorted dependency node identities and their output identities, hashes,
  and byte counts.

Local paths are never part of an action key.

Dependency **outputs**, not dependency recipe keys, are inputs to downstream
actions. If an upstream recipe changes but rebuilds to byte-identical output,
the downstream action key remains reusable on the next planning pass.
Dependency node identity remains part of the key: renaming or replacing a
producer changes the graph edge even when two producers currently emit
identically named bytes.

## Statuses

- `exact-byte-reuse` / `action-and-outputs-match`: the canonical action key
  matches prior state, expected output identities are complete, and every
  current output matches its declared SHA-256 and byte count.
- `rebuild` / `missing-prior-state`: the node has no prior result.
- `rebuild` / `action-key-changed`: recipe, direct input, expected output, or a
  verified dependency output changed.
- `rebuild` / `output-unavailable`: an expected prior output is no longer a
  readable local file.
- `rebuild` / `output-mismatch`: expected output identities are incomplete or
  current output bytes differ from prior evidence.
- `blocked-dependency` / `dependency-not-reusable`: at least one dependency
  must rebuild, so its future output hash is not yet known.

Planning is intentionally iterative. Build the current `rebuild` nodes through
the owner system, measure and update their state, then plan again. A dependent
becomes actionable only after every dependency output is exact and reusable.
REEL never guesses a future output hash or schedules work against stale bytes.

## Deferred semantics

V1 does not provide execution, sandboxing, commands, workers, queues, provider
calls, remote CAS/action caches, state mutation, automatic rollback,
publication, promotion, creative selection, or human approval. It does not
claim SLSA provenance or Bazel-compatible caching.
