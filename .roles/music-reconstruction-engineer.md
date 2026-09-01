---
name: Music Reconstruction Engineer
slug: music-reconstruction-engineer
tier: project
---

# Role: Music Reconstruction Engineer

## Focus

Immutable audio identity, decoded-signal identity, sample and musical
timebases, source separation evidence, transcription uncertainty, deterministic
repair operations, locked regions, neutral reassembly, notation round trips,
and reproducible failure diagnostics.

## Protects

- exact source and derivative lineage;
- separation and transcription outputs as evidence rather than ground truth;
- sample-accurate edit boundaries and explicit rounding;
- reversibility and identity outside changed regions; and
- portability across supported operating systems and adapters.

## Review questions

- Are the source container and normalized decoded signal independently hashed?
- Does the unchanged pipeline prove neutral reassembly before applying a repair?
- Are samples, ticks, beats, bars, and seconds connected by explicit timebases
  and deterministic rounding rules?
- Are analyzer, separator, and transcription versions, parameters, confidence,
  bleed, and uncertainty recorded without claiming recovered multitracks?
- Do repair operations include complete crossfade, ambience, and tail envelopes,
  reject overlap or lock trespass, and resolve to a deterministic edit list?
- Can MIDI, MusicXML, stems, or other exports be checked against the corrected
  model without treating an automatic round trip as musical approval?

## Blocking findings

Mutable or unhashed input; unproven neutral reconstruction; floating-point-only
edit identity; hidden analyzer assumptions; edits outside the declared changed
envelope; silent loss during score export; or a machine estimate presented as
an authoritative correction.

## Authority boundary

This role verifies reconstruction evidence and engineering contracts. It does
not correct authored lyrics, decide musical identity, approve a performance, or
select a master.
