# Explicit lossless still master encoding

The archived still worker always produced FFV1, whose repeated full frames can
exceed a normal job allowance after atomic publication space is included.
Increasing the shared capacity guard would not repair that encoding mismatch.
The managed materialization profile now optionally selects native lossless H.264
with QP 0. Existing callers retain FFV1 and their original v1 receipt contract.
The new choice is recorded in a v2 receipt and the materialization observation.

Producer-applied editing and platform review requires identical geometry,
frame clocks, source colour precision and complete pictures. Sound review
requires unchanged PCM24 and sample clocks. Rights/provenance review preserves
the exact frozen source, existing immutable outputs and private candidate state;
no technical result grants selection, human approval or publication. Full fresh
YUV, displayed RGB and PCM comparison remains mandatory in materialization.
The option grants no capacity increase, cleanup permission or runtime adoption.

Regression coverage includes explicit receipt-version/encoding mismatches,
strict stream contracts and actual archived materialization with the source
checkout unavailable, a full-range detailed JPEG, and independent primary
comparison. Both encoding choices keep existing missing-input, primary-budget,
overwrite and CLI checks. Qualification results are recorded separately.

Local validation passes 408 library tests (16 explicitly ignored), both actual
archived materialization fixtures, formatting and warnings-denied all-target,
all-feature Clippy. The native fixture pair includes both in-process and CLI
execution with original source folders unavailable. Hosted qualification and
actual producer runtime adoption remain pending.
