# Signed approval attestations and C2PA verification (v0.2.38)

REEL v0.2.38 adds two independent, provider-neutral capabilities:

- owner-issued **signed approval attestations** that bind a single human
  decision to an exact target, policy, scope, and authority registry with an
  Ed25519 signature over documented canonical bytes; and
- **C2PA verification** of an asset's Content Credentials through an official,
  externally supplied `c2patool` executable that REEL pins by hash and invokes
  directly.

REEL never mints authority, never authors a manifest, and never installs,
bundles, or discovers a signing key or a verification tool. Signing keys, local
paths, prompts, and raw tool payloads remain outside every portable output. A
valid signature or a valid manifest is reported for exactly what it proves and
never as creative, rights, publication, or release approval.

## Signed approval attestations

### Authority registry

An owner publishes an approval-authority registry
(`reel.approval-authority-registry.v0.1`) that enumerates every authorized
approver:

```json
{
  "schema": "reel.approval-authority-registry.v0.1",
  "owner_id": "owner:example",
  "registry_id": "registry:example-2026",
  "entries": [
    {
      "approver_id": "approver:director",
      "role": "director",
      "public_key": "<lower-case hex Ed25519 public key>",
      "key_id": "<lower-case SHA-256 hex of the public key>",
      "scopes": ["scope:episode-1-final"],
      "decisions": ["approved", "rejected", "revoked"]
    }
  ]
}
```

Each entry's `key_id` must equal the lower-case SHA-256 hex of the decoded
public key. Scopes, roles, and decisions are matched exactly: there are no
wildcards and no case folding.

### Signing

`approval-sign` consumes a strict local authorization contract
(`reel.approval-authorization-input.v0.1`) and writes a no-clobber, path-free
signed attestation (`reel.signed-approval-attestation.v0.1`):

```powershell
cargo run -- approval-sign authorization-input.json `
  --output-path attestation.json --output json
```

The authorization input binds:

- the owner ID and the registry pinned by SHA-256;
- the local Ed25519 signing key path (a raw 32-byte key file);
- the authority-context SHA-256 shared across a decision chain;
- the approver ID, role, and exact decision scope;
- the decision (`approved`, `rejected`, or `revoked`);
- the target kind and the target file pinned by SHA-256;
- the governing policy SHA-256;
- a one-based sequence and canonical UTC issued/expiry timestamps;
- an exact prior attestation (path plus SHA-256) for every non-initial
  decision.

The signing key is read into a zeroizing buffer, used only to produce the
signature, and is never serialized, logged, or copied into any output. The
attestation embeds the **public** key and its derived `key_id`; the private key
never leaves the local key file.

REEL signs documented, deterministic, domain-separated canonical bytes — not
ordinary JSON. The canonical encoding is

```text
chunk(DOMAIN)
  || for each field in fixed order:
       chunk(label) || presence_byte || (chunk(value) when present)
```

where `chunk(x) = big_endian_u64(len(x)) || x` and `DOMAIN` is
`REEL-signed-approval-attestation-canonical-v0.2.38`. Length-delimited framing
makes the encoding unambiguous, and the domain string prevents cross-protocol
signature reuse. The covered fields, in order, are schema, owner ID, registry
ID, registry SHA-256, authority-context SHA-256, target kind, target SHA-256,
policy SHA-256, scope, decision, role, approver ID, public key, key ID, issued
timestamp, expiry timestamp, sequence, and optional prior-attestation SHA-256.
The public key is inside the signed bytes; the signature itself is not.

Every scalar is strictly validated: SHA-256 values are lower-case hex of the
correct length, and timestamps are canonical UTC `YYYY-MM-DDTHH:MM:SSZ` with a
mandatory `Z` suffix, years `0001` through `9999`, and Gregorian
calendar-checked components. Fixed-width timestamps make a lexicographic
comparison identical to a chronological one.

A `revoked` decision always requires an exact, independently validated prior
attestation; an origin decision may only be `approved` or `rejected`.

### Independent verification

`approval-verify` validates a hash-pinned chain against an independently trusted
registry digest (`reel.approval-verification-input.v0.1`):

```powershell
cargo run -- approval-verify verification-input.json `
  --output-path verification-report.json --output json
```

The verification input supplies an `expected_registry_sha256` that the verifier
trusts out of band, the registry file pinned by its own SHA-256, the current
target pinned by SHA-256, an explicit `verification_time_utc`, and the ordered
chain of attestation files each pinned by SHA-256. The verifier:

- rejects the registry if its bytes do not match both the pinned digest and the
  independently trusted expected digest, defeating registry substitution;
- reads each attestation once, hashes those exact bytes, and parses the same
  bytes, rejecting duplicate or unknown JSON fields;
- re-derives the canonical bytes and checks each Ed25519 signature strictly;
- confirms every node is authorized by the registry for its approver, key ID,
  role, scope, and decision;
- requires the complete supplied lineage: a sequence-one origin with no parent,
  then exact one-step sequence increments, increasing issued times, exact
  parent-hash links, legal decision transitions, and no cycles;
- evaluates the head attestation's validity window against the supplied
  verification time only — never a wall clock.

The path-free report states each dimension independently:

```text
cryptographic_signature_valid   the head signature verifies
registry_authorized             the head is authorized by the trusted registry
target_integrity_verified       the current target matches the signed digest
time_valid_at_verification      the supplied time is within the head window
signed_human_decision           the exact signed decision word
historical_decisions            every decision in chain order
current_status_basis            "full-hash-pinned-origin-chain-head"
authenticated_from_origin       true: verification requires the sequence-one origin
```

Signing validates only its direct pinned parent, while verification rejects a
partial lineage rather than treating its head as a current decision.
`current_status_basis` names honestly that the current status is only as
authoritative as the supplied complete hash-pinned chain head; REEL cannot prove
that no later, unsupplied attestation exists. The report always sets
`implies_rights`, `implies_publication`, `implies_release`, and
`implies_other_scopes` to `false`: a signature proves an exact scoped human
decision and nothing beyond it.

## C2PA verification

`c2pa-verify` verifies an asset's Content Credentials through an official
`c2patool` executable that the caller supplies and REEL pins by hash
(`reel.c2pa-verification-input.v0.1`):

```powershell
cargo run -- c2pa-verify c2pa-input.json `
  --output-path c2pa-report.json --output json
```

The input requires an **absolute** `c2patool_path`, an
`expected_c2patool_sha256`, the target file pinned by SHA-256, and an optional
`expected_tool_version`. REEL:

- copies the executable into a private temporary snapshot (retaining `.exe` on
  Windows and executable permissions on Unix), hashes those exact snapshot
  bytes against the pinned digest, and invokes only that snapshot;
- copies the exact target bytes into a private temporary snapshot under a
  generated basename plus the original media extension, hashes that snapshot,
  and rejects any mismatch with the pinned target digest — closing the target
  time-of-check/time-of-use gap without leaking source paths;
- captures and pins the tool version from a bounded `--version` invocation and
  rejects a mismatch with any expected version;
- creates a private network-denied JSON settings file in the snapshot directory
  that disables remote-manifest fetching, OCSP fetching, identity decoding,
  redirects, trust evaluation, and all allowed network hosts, then invokes the
  tool directly with fixed arguments
  `<asset> --settings <private-settings>`, a null stdin, and a cleared
  environment (no shell, no PATH discovery);
- reads stdout and stderr through bounded drains, stops and kills the tool at
  the first limit-plus-one byte, enforces a finite process timeout, and rejects
  a non-zero exit status;
- parses the current official JSON fields strictly: top-level
  `validation_state` must be `Valid`, `validation_results.activeManifest` must
  exist, and bounded code strings are collected from its `success`,
  `informational`, and `failure` arrays. Any non-empty `failure` array,
  `Invalid` state, `Trusted` state, missing state/results, malformed report, or
  duplicate JSON keys is rejected. Legacy-only `validation_status` reports
  are rejected.

The path-free report records the current integrity evidence while keeping trust
explicitly unevaluated:

```text
manifest_integrity   "valid" only when the active manifest validates
validation_state     official c2patool state (`Valid` on accepted output)
validation_status_codes bounded success/informational/failure code strings
certificate_trust    always "not-evaluated" in V1
trust_evaluated      always false in V1
report_sha256        digest of the exact bounded tool output
verifier_settings_sha256 digest of REEL's fixed network-denied settings
```

V1 performs integrity validation only. It does not invoke the official
`c2patool trust` subcommand or load trust resources, and it never claims
certificate trust based on status codes. It also disables automatic remote
manifest and other network retrieval so untrusted media cannot trigger outbound
requests. Future trust remains a separate, explicit hash-pinned input. The
report sets `grants_identity`, `grants_rights`, `grants_publication`, and
`grants_release` to `false`: a verified manifest attests provenance, not
approval.

REEL does not author manifests, does not install or bundle `c2patool`, and does
not search `PATH` for it.

## Human authority boundary

Approval reports and C2PA reports both state:

```text
human_authority_required / human_review_required = true
implies_rights / grants_rights = false
implies_publication / grants_publication = false
implies_release / grants_release = false
```

A valid signature or a valid Content Credential is exactly that. It never
selects an output and never grants creative, rights, publication, or release
approval.
