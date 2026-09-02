# Audio-repair commission acceptance matrix

Date: 2026-09-01  
Branch: `feat/complete-audio-repair-commission`  
Fixture policy: synthetic media and text only

This matrix closes the generic REEL engineering scope. It does not select a
cue, infer instruments, compose or translate creative content, approve a mix,
designate a Golden, or authorize delivery/publication.

## Dialogue-anchored score mixing

| Requirement | Implementation and executable evidence | Status |
|---|---|---|
| Dialogue routing and legacy narration compatibility | Optional `dialogue` role and configurable speech detector bus; legacy `narration_ducking` normalizes through the documented compatibility rule. Covered by manifest/audio-preview unit and all-target regression tests. | Complete |
| Event gain automation | Local-time or beat-marker anchors, exactly-one anchor validation, finite/sorted/unique/in-range resolution, hold/linear/smooth interpolation, and receipt-bound resolved graph. Covered by manifest validation and audio graph tests. | Complete |
| Target-specific ducking | Ordered `audio_ducking` policies with detector/target roles, threshold, ratio, attack, release, and bounded maximum reduction. The real synthetic FFmpeg test proves music attenuation while effects remain unchanged. | Complete |
| D/M/E and full mix | Exact-geometry 48 kHz/24-bit D, M, E, pre-master, mastered full, no-score, mono, and small-speaker outputs with path-free receipts and overwrite protection. The real synthetic FFmpeg test proves sample-level D+M+E recombination. | Complete |
| Stable graph | Enforced order: trim/loop, event gain/automation, fades, role buses, dynamic processing, targeted ducking, bus sum, mastering/limiter, runtime conform. Existing optional-field behavior remains regression-tested. | Complete |
| Speech-keyed dynamic EQ | Portable FFmpeg presence-band carve applies only to declared target roles, keyed only by detector roles, before broadband ducking. The cross-platform synthetic test measures the requested-band reduction and unchanged effects. | Complete |
| Dialogue quality evidence | Policy-driven dialogue-gated loudness, speech-window margin, clipping, duration, stem lineage, mono compatibility, and small-speaker proxy have synthetic pass/fail tests. | Complete |
| Review variants | One manifest produces full, D-only, M-only, E-only, no-score, mono, and small-speaker variants. Receipts identify the variant/policy and never claim approval. | Complete |

Primary command:

```text
cargo test --lib audio_preview::tests::real_dialogue_ducking_stems_recombine_and_recheck -- --ignored --exact
```

The command is pinned in the Linux and Windows CI matrix with synthetic audio.

## Song decomposition, repair, adaptation, and rescoring runway

| Roadmap capability | Implementation and executable evidence | Status |
|---|---|---|
| Source/decomposition contracts | Immutable decoded-PCM identity, provider-neutral evidence intake, semantic import, corrected editable model, complete evidence dispositions, and comparison/selection boundaries. | Complete |
| Deterministic repair | Full repair vocabulary: keep/lock, cut, insert, replace, repeat, move, crossfade, preserve-tail, match-gain, hash-bound match-EQ, and beat-grid extend-bars. Receipts bind exact outside spans, beat alignment, clipping, loudness/ambience, reverb-tail/phase, and spectral seams. | Complete |
| Bounded external re-sing/repaint | Local-only, network-denied, no-download adapter request/plan plus independent full-length candidate checking. Exact outside-region identity, lyric evidence independence, consent, license/model/checkpoint/seed, and rejected-candidate retention are enforced. REEL does not invoke a provider. | Complete |
| Same-music English adaptation | Exact source/translation/performance text layers, authority/decision bindings, prosody mapping, retained-music binding, candidate evidence, and human selection boundary were delivered in v0.3.8-v0.3.9. | Complete |
| Arrangement/rescore candidates | Model-bound, checkpoint-governed arrangement plan/candidate contracts with exact score/audio lineage and no automatic ranking or approval were delivered in v0.3.10-v0.3.11. | Complete |
| Editable score and printable lead sheet | Deterministic MIDI, MusicXML, rehearsal WAV, and optional SVG lead sheet with treble clef, form, harmony, exact lyric underlay, packet receipt, independent round trip, and tamper rejection. | Complete |

Primary repair commands:

```text
cargo test --test music_repair_render_cli_v027 real_ffmpeg_cut_render_is_exact_and_rechecks_retained_evidence -- --ignored --exact
cargo test -p reel-music --test external_repair_contracts
cargo test --test music_score_export_cli_v031
```

## Acceptance and compatibility evidence

- Legacy manifests omit every new field and retain their previous graph and
  score-packet artifact set.
- Invalid, duplicate, unresolved, or out-of-range automation anchors fail.
- Maximum ducking reduction prevents an inaudible target collapse.
- D/M/E files share exact start, channel layout, sample rate, sample count, and
  duration; the recombination residual is checked against the declared
  one-sample tolerance.
- Source, manifest, policy, plan, receipt, and output tampering fail their
  respective checkers; output directories are never overwritten.
- Audio receipt serialization is tested for path leakage.
- Synthetic pass/fail cases cover speech margin, dialogue loudness, clipping,
  mono compatibility, small-speaker proxy, continuity, lyric evidence, and
  lead-sheet underlay.
- CI runs the complete Rust suite, including the engine-neutral repair
  materializer, on Linux and Windows. It separately executes the real FFmpeg
  dialogue/stem path on both systems and the legacy WSL-oriented repair-render
  path on Linux, always with synthetic media.

## Phasing and omissions

No requested P0 or P1 REEL feature is deferred. Real BERTICA cue selection,
lyrics, translations, recordings, mix values, scores, consent decisions,
candidate selection, and human approvals remain intentionally outside this
generic repository and must be supplied through project-owned manifests and
decision artifacts.
