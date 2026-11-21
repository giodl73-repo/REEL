# Independent spoken-edition checking and text access

**Status:** M04 design/preparatory work, stacked on M03. This document specifies
checks to implement, not passed qualification. Dependencies and closure remain
in the [PR plan](media-expansion-pr-plan.md). The
[producer contract](spoken-edition-production-v1.md) defines the candidate input.

## Trust boundary

The checker takes the original declaration, canonical compiled plan, explicit
processing policy (identity if omitted), selected input provider, candidate
provider and an independently supplied expected runtime identity. The normal CLI
uses its actual executable identity; earlier-runtime admission/reopening is M05
work. Plan and receipt runtime fields cannot substitute for that expected pin.

The candidate receipt is untrusted, bounded control data. A checker must:

1. Recompile original controls and selected metadata with the expected runtime;
   compare exact canonical plan bytes and all recipe/placement identities.
2. Strictly parse the local producer receipt, reject unexpected fields, and
   require the producer's actual schema, fixed status fields and edition order.
3. Reopen complete selected WAVE inputs and candidate WAVE outputs. Recheck
   source/container pins, exact PCM24 format and frame counts, and output pins.
4. Independently calculate source selection, rational gain, linear envelope,
   gap and overlap samples from the original controls. Compare **every** decoded
   output sample with zero tolerance. Do not call the producer, reuse its
   transformed buffer, or accept its hashes as the waveform oracle.
5. Recalculate selected/processed PCM and execution recipe identities; verify
   private receipt fields against these results. Require the profile peak,
   aggregate input/output/workspace limits and cooperative deadline.
6. Derive speaker-aware text and chapters from the checked edition clock. When
   an access package is supplied, rederive its exact contents and compare them,
   including text, order, speaker roles, source/recipe bindings and intervals.

Sharing strict schema types, bounded WAVE parsing, declaration compilation and
hashing is allowed. Sample processing must have a separate implementation and
independent golden tests. Rehashing a false receipt or access manifest after
tampering must still fail semantic or PCM comparison.

## Commands and allowed effects

```sh
reel spoken-edition-check declaration.json plan.json \
  --asset-root staged-inputs --candidate candidate

reel spoken-edition-check declaration.json plan.json \
  --asset-root staged-inputs --candidate candidate --write-access new-access

reel spoken-edition-check declaration.json plan.json \
  --asset-root staged-inputs --candidate candidate --access existing-access
```

`--processing policy.json` supplies the independently authored policy. Never infer
the expected gain/fade policy from the receipt being checked. `--access` verifies
an existing package; `--write-access` creates one at a new destination only after
all candidate checks pass. These options are mutually exclusive. Read-only mode
writes no file. Failure emits no success report or asset promotion. Creation
stages private text and commits a no-clobber final manifest last; interruption
leaves no usable access package and preserves unrelated files.

CLI stdout is a portable count/status summary. Exact source/output/recipe hashes,
IDs and text remain in local private evidence. No provider, native publication,
storage repair, player, upload or network operation is started.

## Text and chapter model

The private `reel.spoken-access.v1` package binds each edition's language,
compiled/execution recipe, checked WAVE identity and exact sample duration.
Transcript entries are half-open integer sample intervals at 48,000 Hz:

- A spoken entry uses exact selected script text with `text_basis` explicitly
  identifying that layer. As-performed and recognition evidence stay separate;
  recognition spelling never silently rewrites the script. This is checked
  metadata correspondence, not acoustic proof that the recording said it.
  Word evidence is separately tagged by its `evidence_text`, ordinal speaker
  and exact shifted sample interval, preserving each language's native clock.
- A controlled engineering signal is labeled as non-speech. It must not present
  the fixture's example script as words observed in a tone.
- An added gap gets a non-speech pause entry. Recorded leading/trailing silence
  remains inside its selected cue, so it is not counted a second time.
- Intentional overlaps remain overlapping entries with explicit overlap length;
  no word interval is silently shortened to fit a subtitle convention.

Public-facing speaker labels use ordinal roles in first-appearance order within
each edition. No performer/voice/provider identity is copied into transcript
labels. Private source bindings preserve the exact selected role correspondence.
Module chapters use the compiler's checked language-local start/end samples;
overlapping modules may have overlapping chapter intervals. This is chapter data,
not an implied EPUB, YouTube or player export.

The package contains structured text/chapter JSON and a simple UTF-8 transcript
for each edition. Both are derived and checked against exact controlled content.
They are local private artifacts, not an approved public release. Scope and
translation decisions remain with the producing project.

## Limits and required evidence

Use M03's engineering/spoken duration, PCM format, source, output, workspace and
peak limits. Bound the plan, private receipt and access manifest individually to
16 MiB, processing policy to 256 KiB and aggregate text artifacts to 16 MiB.
Default deadline is 30 seconds; hosts may narrow it, never enlarge the profile.
Check cancellation during bounded reads, recomputation and final publication.
Trusted synchronous providers must return; this is not a sandbox for hung I/O.

Run on Windows and Linux, with exact zero-tolerance comparisons of native
lossless PCM. No external decoder is required for this deliberately narrow RIFF
profile. An unavailable platform is not run, not a pass.

| Required case | Observable result / oracle |
| --- | --- |
| `checker_reopens_media_and_rejects_rehashed_tamper` | All four real output files are reopened; altered PCM plus updated superficial hashes still fails independently calculated samples |
| `four_transcripts_and_chapters_follow_edition_clock` | Hand-calculated EN/ES intervals, exact selected text, ordinal roles, added pauses and chapter bounds; signal-only fixtures are labeled non-speech |
| `portable_output_redacts_canaries` | Real CLI success/failure summaries contain no private path, source, role, text or authority canary |
| Source/runtime/plan/processing mutations | Changed source, order, placement, gain, runtime or role fails despite recomputed shallow report hashes |
| Access tampering | Altered/reordered text, role, interval, chapter or source binding fails, including a rehashed access manifest |
| Closure and effects | Missing receipt/media, existing destination, injected timeout/interruption and invalid candidate produce no success/promotion; old candidate and unrelated files survive |

The owning integration target is `spoken_programme_media`; M03 cases remain.
Run relevant clock/compiler and shared PCM/WAVE regressions, all-feature Clippy,
formatting, `git diff --check` and required hosted checks. Record exact invocations,
platform/runtime and results in M04's role review. Sound, Platform/Audience,
Vocal Adaptation and Rights are the required checklist lenses.

Independent technical success leaves acoustic word fidelity, naturalness,
translation/voice authority, listening, selection, delivery and adoption unpassed.
