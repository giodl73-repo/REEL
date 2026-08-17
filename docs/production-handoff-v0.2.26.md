# Production-bound handoffs — v0.2.26

v0.2.26 connects choreography and cross-department craft planning to the exact
production revision they describe. The integration remains additive: existing
production manifests and sidecars without a binding retain their earlier
behavior.

## One timing and identity spine

Both sidecars may carry `reel.production-binding.v0.1` data with:

- an exact production-manifest SHA-256 and work ID;
- local sidecar shot references mapped to production shot IDs;
- local sidecar beat references mapped to existing production beat markers.

Validation loads and validates that exact manifest and rejects stale hashes,
unknown work, shots, or beats, choreography duration drift, beat-time drift,
and protected holds longer than their bound shot. Resolved reports are
path-free and contain only hashes, IDs, and timing.

The sanitized fixture at
`manifests/fixtures/shared-production/manifest.yaml` is shared by the
choreography and craft-plan examples.

## Choreography execution

Camera `hold`, `follow`, `whip`, and `settle` phrases use the same named beat
ranges as performer and prop action. The abstract preview renders their framing
intent. A separate `reel.choreography-assets.v0.1` binding pins the choreography
source hash and assigns real background, performer-pose, and prop sprite assets.

```powershell
cargo run -- choreography-sprite-manifest `
  manifests/fixtures/choreography/simple-handoff.yaml `
  manifests/fixtures/choreography/assets.yaml `
  --output-path target/handoff-production.yaml --output json

cargo run -- animatic-render target/handoff-production.yaml `
  --asset-root manifests/fixtures/choreography/assets `
  --silent --no-captions --output target/handoff.mp4
```

The compiler emits a validated production manifest consumed by REEL's existing
sprite renderer. Its lineage binds the source choreography hash, asset-binding
hash, production-manifest hash, and exact shot ID. Pose swaps, paths, handoff,
and camera direction remain explicit rather than inferred by the renderer.

## Department sharing boundary

Every craft evidence and asset record declares `internal-only`,
`approval-required`, or `shareable`. Policy is enforced only when the packet is
requested with `--distribution external`; local planning is not burdened with
an external ceremony. Approval-required material needs a non-empty approval
reference, while internal-only material cannot cross that boundary.

`department-packet-receipt` writes a path-free receipt containing the packet
schema, SHA-256, byte count, source-plan hash, department, and distribution
scope. `department-packet-check` rejects changed bytes or mismatched metadata.
Neither command claims license clearance or human approval.

## Authority boundary

REEL validates identity, timing, routing, declared distribution policy, and
artifact integrity. It does not direct performance, judge artistic quality,
grant rights, approve reconstruction, or decide whether evidence is culturally
or historically sufficient.
