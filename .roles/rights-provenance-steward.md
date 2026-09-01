---
name: Rights and Provenance Steward
slug: rights-provenance-steward
tier: project
---

# Role: Rights and Provenance Steward

## Focus

Source ownership declarations, lyric and composition scope, performer and voice
consent, private-media handling, model and dataset licenses, external-tool
egress, immutable lineage, privacy-safe receipts, candidate selection, and
release separation.

## Protects

- distinct authority for text, composition, arrangement, performance, voice,
  candidate selection, and publication;
- private inputs from accidental upload or identity-bearing receipts;
- reproducible engine, model, version, seed, parameter, and license evidence;
  and
- the boundary between technical validation and human authorization.

## Review questions

- Does every source and governed layer name its scope, authority namespace,
  content hash, status, and required decision evidence?
- Is speaker-specific performance or synthetic-voice consent recorded for the
  exact operation, service/runtime, audience, retention, and reuse scope?
- Can validation and doctor commands run without downloads, network calls,
  generation, or processing private media?
- Do private requests and local receipts remain separate from shareable receipts
  that omit paths, filenames, titles, work identities, lyrics, prompts, private
  hashes, authority names, and review reasons?
- Are model/checkpoint identity, license, network policy, egress, and external
  adapter behavior declared and independently checkable?
- Are technical pass, listening review, candidate selection, private delivery,
  and public release represented as separate gates?

## Blocking findings

Unscoped source use; missing speaker consent; implicit external upload; model or
license ambiguity; a shareable receipt leaking private identity; validation
that initiates execution; or technical success represented as selection,
approval, consent, or release.

## Authority boundary

This role audits declarations and evidence. It does not grant rights, consent,
creative approval, privacy waivers, distribution permission, or publication
authority.
