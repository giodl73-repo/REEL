---
skill: faces-development-loop
topic: reel-changed-only-build-v0.2.41
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.41 changed-only build roles check

## Frame

**Working owner system:** BERTICA already has deterministic scripts that build
its S1E01 shot register, manifest, backlog, and treatment from exact source
files. REEL's picture planner already performs still-specific recipe and cache
checks.

**Missing shared capability:** cross-department scripts have no common,
portable way to prove which build nodes remain reusable after one source or
recipe changes.

**Thesis:** REEL can validate an owner-authored dependency graph and produce a
changed-only plan from exact current bytes without becoming a build executor,
cache server, scheduler, or creative authority.

**Deletion target:** BERTICA can retire one hand-maintained stale/downstream
rebuild checklist for its S1E01 production chain.

**Disproof:** the slice fails if a changed or missing current file is reused, a
cycle or undeclared dependency is accepted, downstream work runs against an
unknown new dependency output, paths leak into portable output, or REEL must
execute a consumer script.

## Audit and comparison

- REEL's picture planner hashes a manifest, recipe inputs, output profile, and
  tool version, then verifies current cached image bytes before exact reuse.
  **Reuse:** content keys and current-byte verification.
- Provider resume validates complete parent lineage before reuse.
  **Adapt:** graph dependencies require deterministic topological validation.
- Production state and readiness reports keep technical status separate from
  human approval. **Reuse:** planning never implies execution or approval.
- Picture-specific dimensions/media probing do not generalize to scripts,
  contracts, audio, packages, or manifests. **Avoid:** do not widen the picture
  cache schema.

Bazel's official cache model separates action hashes from content-addressed
output files and requires declared inputs/outputs. SLSA provenance separates
build definitions, resolved dependencies, execution details, and output
subjects. REEL adopts only the smallest shared semantics: declared current
bytes, content-derived action keys, and output-byte verification.

- <https://bazel.build/remote/caching>
- <https://slsa.dev/spec/v1.1/provenance>

V1 deliberately does not implement Bazel execution, CAS transport, remote
caching, sandboxing, or SLSA attestations.

## Role findings

### Editor

- **P2:** A changed timing or edit contract must invalidate only its node first;
  downstream renders remain blocked until the new exact output is known.
- **P2:** “Latest” is not “reusable.” Reuse requires the same action key and
  current output bytes.
- **P3:** The plan needs stable reasons, not only a boolean stale flag.

### Animation Director

- **P2:** Recipe, model/config contract, continuity inputs, and asset inputs are
  independent declared bytes and all must affect the action key.
- **P2:** An upstream rebuild may produce identical bytes. Downstream action
  keys therefore bind dependency output hashes, not dependency recipe hashes.

### Platform and Audience

- **P2:** Missing, changed, and blocked work must remain distinct for reliable
  operator decisions.
- **P3:** V1 is local and path-free; remote cache protocols and fleet rollout
  would add security and operational ownership not required for the proof.

### Story Director

- **P2:** Source prose stays in owner files. REEL sees paths locally but emits
  only portable IDs, hashes, action keys, and normalized reasons.
- **P2:** Technical reuse cannot imply story approval or preserve a stale
  creative decision.

### Sound Designer

- **P2:** The graph must be media-generic so voice, score, effects, picture,
  captions, and packages use the same dependency semantics.
- **P2:** No implicit dependency discovery: undeclared audio or mix inputs
  would make reuse dishonest.

## Security and simplicity conditions

1. Strict graph and prior-state schemas reject unknown fields.
2. Node IDs, operation kinds, input/output IDs, and dependencies are bounded
   portable tokens.
3. Recipe, direct input, and reusable output files are read and hash-verified
   from current bytes.
4. Graphs reject duplicate nodes, duplicate inputs/outputs, unknown
   dependencies, self-dependencies, and cycles.
5. Action keys use canonical sorted identities and exact SHA-256 values.
6. A node is reusable only when its prior action key matches and all current
   output bytes match. Missing or changed outputs require rebuild.
7. If any dependency is not reusable, the node is `blocked-dependency`; REEL
   does not guess the dependency's future output hash.
8. Output is path-free, deterministic, no-clobber, and never executes commands,
   mutates caches, selects media, or grants approval.

## Verdict

**APPROVED-WITH-CONDITIONS.** V1 is a planner over exact local evidence, not a
general build graph engine. The required proof is a real BERTICA S1E01 slice
where unchanged upstream outputs reuse, one changed source rebuilds its direct
node, and only its downstream dependents block.

## BERTICA S1E01 proof

The real proof graph used:

- BERTICA's `build_s1e01_measured_visual_conform.py` as the upstream recipe;
- its five current cue, performance, conform, timeline, and continuity inputs;
- its current shot-register, generation-backlog, visual-treatment, and REEL
  manifest outputs;
- the current REEL CLI as the downstream OTIO recipe;
- the exact 70-clip offline S1E01 OTIO output.

With complete prior state, both nodes reported `exact-byte-reuse` and
`action-and-outputs-match`. The path-free plan is 3,708 bytes with SHA-256
`3f2659fb2bda0a48360b369560046febf9623174647bd59ca42c9c7edd08571b`.

A private copy of the real cue-map input was changed by one trailing byte,
rehash-declared, and planned against the same prior state. The conform node
reported `rebuild` / `action-key-changed`; only the dependent OTIO node reported
`blocked-dependency` / `dependency-not-reusable`. The changed plan is 2,746
bytes with SHA-256
`bfba56d53f8a32120dffe2ec482129d37a0e3c034f773360a1c38f44a8457f55`.

Neither portable plan contains a local path, URL, source text, prompt,
credential, command, or creative approval claim.
