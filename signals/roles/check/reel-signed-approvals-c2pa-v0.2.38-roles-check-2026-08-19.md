---
skill: roles-check
topic: reel-signed-approvals-c2pa-v0.2.38
date: 2026-08-19
roles_used: 5
p1_count: 0
verdict: APPROVED-WITH-CONDITIONS
---

# REEL v0.2.38 signed approvals and C2PA roles check

## Artifact identification

- Type: proposed owner-issued approval attestations and C2PA verification
- Working owner systems: production repositories retain reviewer identity,
  authority policy, creative judgment, rights, publication, and release.
- REEL contribution: cryptographic binding, registry-scoped verification,
  C2PA integrity evidence, and path-free technical reports.
- Preserved invariant: existing REEL review records continue rejecting claims
  of authentication, signature, consent, or approval.

## Role findings

### Story Director

- **P2:** A valid signature proves control of a key, not narrative authority.
  Verification must require an exact owner-controlled authority registry that
  binds the key to an approver, role, and allowed scope.
- **P2:** Approval must bind the exact target and policy hashes. It must not
  float to a revised story, shot, cut, or artifact.
- **P3:** Private rationale remains in the owner system; portable attestations
  carry only bounded decision metadata.

### Editor

- **P2:** Attestations are immutable. Rejection or revocation must be a new
  signed child bound to the exact prior attestation hash, never mutation.
- **P2:** Approval of one scope must not imply another. Creative, rights,
  publication, and release scopes remain distinct.
- **P3:** Verification reports must expose the exact target, policy, registry,
  and attestation hashes used.

### Animation Director

- **P2:** C2PA validation establishes manifest and asset integrity, not visual
  continuity, quality, likeness, or creative approval.
- **P2:** Content Credentials may contain provider or edit provenance that is
  useful evidence but must not be flattened into a generic “approved” flag.
- **P3:** Raw C2PA reports remain local; portable output contains hashes,
  bounded status codes, tool version, and manifest identity only.

### Sound Designer

- **P2:** Signed approval must be media-generic so exact audio, mix, music, and
  silence artifacts can use the same envelope.
- **P2:** A C2PA-valid audio asset still requires listening, performance,
  timing, and rights review.
- **P3:** Scope names must be owner-defined portable identifiers rather than a
  hard-coded picture-only taxonomy.

### Platform and Audience

- **P2:** C2PA integrity validation is the V1 result; certificate trust is
  deliberately not evaluated. REEL does not invoke `c2patool trust` or load
  trust resources, and future trust requires an explicit hash-pinned input.
- **P2:** C2PA verification must use the official `c2patool` executable
  directly, capture its exact version and report hash, and avoid a shell.
- **P2:** Successful C2PA validation does not grant identity, rights,
  publication, platform acceptance, or release.

## Synthesis

Roles reviewed: 5  
P1 blockers: 0 | P2 issues: 11 | P3 notes: 5

Verdict: **APPROVED-WITH-CONDITIONS**

## Normative v0.2.38 slice

1. Add a separate owner-issued Ed25519 attestation contract; do not widen
   existing unsigned review records.
2. Bind target hash, policy hash, scope, decision, approver, authority role,
   signing timestamp, and optional prior attestation hash.
3. Derive signer identity from the public key and require an exact
   owner-controlled registry entry authorizing approver, role, and scope.
4. Support `approved`, `rejected`, and `revoked`; revocation must cite and
   validate the exact prior attestation.
5. Keep private keys local and accept only a raw 32-byte Ed25519 signing key;
   never serialize or log key material.
6. Provide independent signature and registry verification against the current
   target bytes.
7. Verify C2PA through the official `c2patool` executable with a fixed private
   network-denied settings file, record its version, hash the raw report and
   settings, require the current `validation_state` and active manifest
   results, and reject validation failures.
8. Distinguish integrity validation from deliberately unevaluated certificate
   trust and human authority.
9. Emit only path-free, no-clobber portable outputs.

## Security review requirements

The pre-implementation security review found no reason to abandon the slice,
but elevated the following conditions to release requirements:

1. The verifier must receive an independently trusted expected registry digest.
   A registry supplied beside an attestation cannot establish its own trust.
2. Sign domain-separated, length-delimited canonical bytes derived from strict
   JSON fields; never sign ordinary pretty JSON or caller-selected bytes.
3. Include the exact public key in the signed body, derive its key ID, and
   require a strict owner-registry match for key, approver, role, and scope.
4. Bind an owner ID, registry ID and digest, authority-context hash, target
   kind and hash, policy hash, exact scope, decision, sequence, UTC issuance
   and expiry, and prior-attestation hash.
5. Validate full revocation/supersession chains, legal state transitions,
   context equality, monotonic sequence/time, cycle absence, and exact parent
   hashes. Historical signature validity alone is not current authorization.
6. Parse one bounded read of each contract and reject duplicate or unknown
   fields. Hash and inspect the same bytes.
7. Require an absolute `c2patool` executable path and expected executable hash;
   invoke it directly with fixed arguments, null stdin, restricted environment,
   bounded stdout/stderr, and no caller-supplied flags.
8. Run C2PA verification against an immutable private snapshot of the exact
   bytes being reported and disable remote-manifest, OCSP, identity, redirect,
   and all other network retrieval.
9. Report manifest integrity, deliberately unevaluated certificate trust,
   registry authority, and human decision as separate statuses.
10. Keep raw C2PA output local and emit only bounded identifiers, hashes,
    counts/status codes, tool version, and executable hash.

## Non-goals

- REEL does not create reviewer identities, decide who has authority, manage
  certificates, author C2PA manifests, fetch trust lists, or publish media.
- A signature or valid Content Credential does not imply truth, consent,
  rights, creative quality, publication permission, or release permission
  outside the exact signed scope.

## Required proof

- One signed approval verified against an exact target and registry.
- One rejection and one revocation chain.
- Wrong target, wrong policy, unauthorized scope, changed registry, changed
  signature, and changed prior-attestation failures.
- C2PA parser tests modeled on official `C.json` Valid output, XCA Invalid
  output, missing state/results, legacy `validation_status: null`, and the
  controlled no-trust rejection of `Trusted`.
- One optional live check with the official `c2patool` when installed.
