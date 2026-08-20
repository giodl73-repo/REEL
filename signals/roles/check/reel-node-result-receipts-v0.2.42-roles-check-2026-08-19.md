---
skill: faces-development-loop
topic: reel-node-result-receipts-v0.2.42
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.42 immutable node-result receipt roles check

## Frame

**Working owner system:** BERTICA, KARTS, and ICELINES already own their build
scripts, execution environments, success criteria, and produced files. REEL
v0.2.41 validates the desired graph and decides changed-only reuse from exact
current bytes.

**Missing shared capability:** after an owner executes one planned node, it
must hand-author a path-rich prior-state entry with the action key, output
hashes, and byte counts before REEL can reuse the result.

**Thesis:** REEL can independently regenerate the exact plan, verify
owner-produced output bytes, emit a portable immutable result receipt, and
advance local changed-only state without running the build or judging its
creative result.

**Deletion target:** remove manual JSON editing and manual output hashing from
the v0.2.41 state-update workflow.

**Disproof:** the slice fails if it records a forged or stale plan, accepts a
blocked/reused node as newly executed, accepts incomplete or aliased outputs,
advances a different prior-state lineage, leaks paths into the receipt,
silently overwrites state, or implies execution/selection/approval by REEL.

## Audit and comparison

### Internal analogues

- Provider-attempt receipts accept sanitized facts from an owner-controlled
  adapter, independently verify captured bytes, bind parent lineage, and keep
  provider execution and approvals false. **Reuse.**
- Changed-only planning already owns strict graph/state schemas, canonical
  action keys, exact current-byte checks, and no-clobber publication. **Reuse
  and extend in the same module.**
- Selection-lock packets atomically group multiple files but also perform an
  explicit creative selection. **Avoid:** result recording is not selection.
- Production package receipts recompute their source before comparing a
  receipt. **Adapt:** regenerate the exact changed-only plan rather than trust
  a supplied plan document.

### External comparators

- Bazel Remote Execution `ActionResult` records output digests and execution
  metadata. **Adapt output digests; avoid workers, commands, CAS, and remote
  execution.**
- SLSA provenance separates build definition, resolved dependencies, run
  details, builder identity, and output subjects. **Adapt the separation
  between planned action and observed outputs; avoid claiming SLSA provenance
  or trusted-builder identity.**
- in-toto link metadata binds materials and products bit-for-bit and explicitly
  supports separating metadata recording from execution. **Reuse separation;
  avoid commands, stdout/stderr, signatures, and supply-chain policy in V1.**

Primary sources:

- <https://github.com/bazelbuild/remote-apis/blob/main/build/bazel/remote/execution/v2/remote_execution.proto>
- <https://slsa.dev/spec/v1.1/provenance>
- <https://github.com/in-toto/attestation/blob/main/spec/v1/statement.md>
- <https://github.com/in-toto/specification/blob/master/in-toto-spec.md>

## Role findings

### Editor

- **P2:** Recording a render or conform result cannot label it latest, locked,
  selected, or editorially approved.
- **P2:** Replaying a receipt must remain tied to the exact prior-state
  lineage; otherwise an older cut could silently become current.

### Animation Director

- **P2:** Multiple byte-distinct outputs for the same action are possible in
  nondeterministic pipelines. The owner chooses which receipt advances state;
  REEL must never compare or choose between them.
- **P2:** Every expected output identity must be present exactly once, and one
  physical file cannot satisfy multiple output identities.

### Platform and Audience

- **P2:** The shareable receipt must contain identifiers, hashes, byte counts,
  and authority boundaries but no local paths, commands, logs, or source text.
- **P3:** Local state may retain paths because it is operational input, not a
  portable evidence artifact.

### Story Director

- **P2:** Action and output identity provide technical lineage only. They
  cannot establish canon fidelity, adaptation quality, or story approval.
- **P3:** Owner result references should be SHA-256 values so private job names
  or production descriptions do not leak.

### Sound Designer

- **P2:** The contract must stay media-generic: exact output bytes can represent
  audio, picture, captions, manifests, packages, or reports.
- **P2:** State advancement cannot imply mix selection, performance approval,
  or delivery readiness.

## Security and simplicity conditions

1. Result input, receipt, plan, graph, and state use strict versioned schemas.
2. Receipt generation regenerates the plan from the exact graph and prior
   state, byte-compares it with the supplied plan, and re-verifies all current
   graph/prior-state evidence.
3. Only a `rebuild` node with a concrete action key can produce a new result
   receipt; `blocked-dependency` and `exact-byte-reuse` are rejected.
4. The owner explicitly returns the planned action key, a hashed private result
   reference, a completed outcome, and one local path per expected output.
5. REEL independently measures output SHA-256 and byte count, rejects missing,
   duplicate, aliased, or directory outputs, and emits no paths.
6. State advancement independently regenerates and byte-compares the expected
   receipt from the graph, prior state, plan, result, and current outputs. It
   then requires the exact prior-state hash, canonicalizes local paths, rejects
   physical aliases including hardlinks, and records the receipt SHA-256.
7. Receipt and state are separate atomic no-clobber writes. A valid receipt can
   survive a failed state publication and be retried; no two-file transaction
   is claimed.
8. State v0.2 remains readable by the planner, while v0.1 state remains
   supported for migration.
9. Execution, success semantics, caching, scheduling, selection, promotion,
   approval, publication, release, and rollback remain outside V1.

## Verdict

**APPROVED-WITH-CONDITIONS.** Prove the slice against the real BERTICA S1E01
conform node: generate the initial rebuild plan, record the four exact outputs,
advance empty state without manual hashes, then obtain exact-byte reuse from
the generated state. Tampering any output after receipt creation must block
state advancement.

## BERTICA S1E01 proof

The proof used BERTICA's real
`build_s1e01_measured_visual_conform.py`, its five current source contracts,
and its four current outputs: shot register, generation backlog, visual
treatment, and REEL production manifest.

Starting from empty state v0.1:

1. REEL planned `measured-visual-conform` as `rebuild`.
2. The owner result input returned only the action key, one sanitized result
   reference, and four output ID/path bindings. It supplied no output hashes or
   byte counts.
3. REEL regenerated the exact plan from current evidence and emitted a
   four-output path-free receipt.
4. REEL advanced state to v0.2 with the receipt SHA-256 bound on the node.
5. Planning from that generated state returned `exact-byte-reuse` /
   `action-and-outputs-match`.

Proof artifacts:

- receipt: 1,618 bytes, SHA-256
  `8249e16b4f1e301f474c76349eca9f288b3b460f39aa9726c0a8765bd7f5798c`;
- advanced local state: 1,608 bytes, SHA-256
  `18c21609619d68a55f0ea56a89c5ba89dd983a864302954e4252d500d695e43a`;
- reuse plan: 2,904 bytes, SHA-256
  `9e3688713d86e45c8db1fd6fc7c2c39cb43340ac0918a7b439fb3999ae48b14e`.

The receipt contains no local path or source content. Rebinding the manifest
output identity to a private one-byte-modified copy caused independently
regenerated receipt evidence to differ from the supplied immutable receipt; no
state file was published.
