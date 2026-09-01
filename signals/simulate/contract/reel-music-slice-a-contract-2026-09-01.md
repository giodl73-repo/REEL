---
skill: simulate-contract
topic: reel-music-slice-a
date: 2026-09-01
gate_result: PASS
---

# Contract simulation: `reel-music` Slice A

## Inputs

- Specification: `docs/music-reconstruction-crate-proposal.md`, especially
  “Slice A — crate and source/repair planning foundation.”
- Implementation: `crates/reel-music/`, root CLI v0.2.26 integration,
  `manifests/fixtures/music-repair-foundation/`, and the v0.2.26 tests/docs.
- Actual execution: full workspace tests on Windows and WSL/Linux, Clippy with
  warnings denied, formatting, role-schema validation, and fixture CLI checks.

## Gate token

```yaml
census-distribution: shared
gate-provenance: §S5.5-Sub-task-A
gate-status: PASS
attestation-by: Music Reconstruction Engineer
attestation-result: All Slice A contract elements are present and executable.
verification-by: Rights and Provenance Steward
verification-result: Independent evidence confirms private synthetic inputs, explicit authority boundaries, and no execution or egress side effects.
mechanism-distribution: shared
mechanism-type-shared: PASS
```

The census and mechanism distributions both resolve to `shared`: contract
behavior is owned by `reel-music`, while process execution remains in the root
`reel` CLI. The attestation names are installed role lenses, not human approval.

## Element diff

| # | Spec element | Actual implementation evidence | Severity | Result |
|---|---|---|---|---|
| 1 | Restore the pre-existing exact-lyric fixture baseline. | `.gitattributes` pins `*.txt` to LF; fixture bytes and hash were updated; all v0.2.25 song tests pass on Windows and Linux. | P2 | Match |
| 2 | Create `crates/reel-music` as a workspace library. | Root workspace includes `crates/reel-music`; crate v0.1.0 builds independently. | P2 | Match |
| 3 | Keep `reel` as the public CLI facade. | Root package depends on `reel-music`; only root `src/main.rs` defines commands. | P2 | Match |
| 4 | Avoid heavyweight DSP/model dependencies. | Crate depends only on existing REEL baseline libraries; no DSP, decoder, model, SDK, or network library was added. | P2 | Match |
| 5 | Use integer sample and musical timebases. | `AudioTimebase`, `MusicalTimebase`, `RoundingMode`, and half-open `SampleRange` validate exact bounds. | P2 | Match |
| 6 | Provide canonical hashing helpers. | Recursive sorted-key canonical JSON hashing is separate from raw manifest hashing; YAML-order test passes. | P2 | Match |
| 7 | Dispatch strict versioned schemas. | Source, neutral, and repair schemas are exact constants; structs reject unknown fields where structurally applicable. | P2 | Match |
| 8 | Validate immutable source identity. | Source validation checks exact bytes, raw-PCM decoded identity, declared byte count, format, timebase, and both hashes. | P2 | Match |
| 9 | Preserve authority and egress boundaries. | Source contract requires namespace/artifact/content/status/roles and decision refs; foundation requires private, network-denied, no-upload use. | P2 | Match |
| 10 | Write a deterministic neutral plan without overwrite. | `music-neutral-plan` atomically persists one full-range keep and lock and refuses an existing output. | P2 | Match |
| 11 | Prove neutral decoded-PCM equality. | `music-neutral-check` revalidates source/plan and requires exact candidate hash and byte length; changed candidate test fails. | P2 | Match |
| 12 | Validate a typed minimal-repair grammar. | Typed variants cover keep/cut/insert/replace/repeat/move/crossfade/tail/gain/EQ/bar-extension/lock. | P2 | Match |
| 13 | Lock every unaffected sample. | Changed envelopes and locks must be ordered, disjoint, and cover the complete source exactly once. | P2 | Match |
| 14 | Reject ambiguous or trespassing edits. | Unique IDs, bounded ranges, verified assets, complete operation coverage, lock trespass rejection, and mutating-overlap rejection are enforced. | P2 | Match |
| 15 | Route required specialist roles. | Repair validation requires reconstruction, sound, editor, and rights/provenance roles. | P3 | Match |
| 16 | Use only synthetic fixtures. | Checked-in 62-byte unsigned PCM and generated test PCM contain no consumer content; documentation states the boundary. | P2 | Match |
| 17 | Prove Windows/Linux portability. | The full workspace suite and the pinned canonical fixture hash pass under Windows and WSL/Linux using isolated targets. | P2 | Match |
| 18 | Keep deferred capabilities visibly deferred. | v0.2.26 docs state that rendering, seam measurement, analysis, notation, language, arrangement, and ACE remain later slices. | P3 | Match |

## Mismatches

No Slice A contract mismatch was found. Two intentionally deferred items are
not failures: container decoding is an adapter concern after raw-PCM identity,
and overlapping operation groups remain unsupported until their composition
semantics are specified.

## Gate result

**PASS** — the implementation matches the reviewed Slice A contract. This gate
does not approve Slice B, a creative repair, a translation, a musical
arrangement, use of private media, or release.
