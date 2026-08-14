# BERTICA request: executable cue-level voice performance direction

Date: 2026-08-13
Requested next version: v0.2.15
Source project: private BERTICA production; no private audio, manuscript text or
identity-bearing asset is included in this request.

## Production finding

The Mayabeque proof established that correct identity, text, pace, loudness and
punctuation do not guarantee a dramatically truthful read. A Cuban family
argument remained too even. In particular, an explosive interruption equivalent
to `¡Cállate, carajo!` and the suspense hinge equivalent to `se tiró al río`
were stored as prose direction but not represented as executable, auditable
performance changes.

REEL currently has speaker-level `performance_direction` and timed narration
cues. It needs a provider-neutral performance sidecar at sub-cue span level,
analogous to its caption-presentation sidecar: exact delivery intent must remain
separate from canonical text and from any engine-specific translation.

## P0 — `reel.voice-performance.v0.1` sidecar

Add a strict YAML contract keyed to existing manifest/cue IDs:

```yaml
schema: reel.voice-performance.v0.1
manifest_sha256: <sha256>
language: es-CU
directing_context:
  register: intimate-family-storytelling
  constraint: heightened but not caricatured
cues:
  - cue_id: cue-amado-interruption
    spans:
      - id: blow-01
        text_sha256: <hash of exact source substring>
        start_char: 0
        end_char: 18
        action: explosive-interruption
        intensity: 0.95
        pace: fast
        pitch_shape: sharp-rise-drop
        energy: high
        onset: hard
        pause_before_ms: 0
        pause_after_ms: 180
        stress_tokens: [callate, carajo]
      - id: danger-appeal
        text_sha256: <hash>
        action: fear-driven-warning
        intensity: 0.82
        pace: urgent
        pitch_shape: rising-question-fall
        energy: high
        pause_after_ms: 220
        stress_tokens: [movemos, hundimos]
```

Required controlled vocabularies should include at minimum:

- `action`: neutral-narration, intimate-recollection, comic-aside,
  breathless-plea, exasperated-demand, explosive-interruption,
  wounded-dignity, precise-counterattack, dangerous-threat,
  fear-driven-warning, suspense-build, suspended-decision, physical-effort,
  astonished-release, dry-comic-button;
- normalized `intensity`, `energy` and optional `breathiness`;
- `pace`, `pitch_shape`, `onset`, `stress_tokens` and protected pauses;
- an `es-CU` register tag without claiming that one stereotyped delivery
  represents every Cuban speaker.

The sidecar must not change, duplicate or normalize canonical narration text.
Character offsets and hashes must bind every span to exact cue text. Reject gaps,
overlaps, unknown cues, stale hashes, invalid stress tokens and contradictory
pause declarations.

## P0 — engine adapter receipt

Add an engine-neutral compilation command:

```powershell
reel voice-performance-plan manifest.yaml performance.yaml \
  --engine chatterbox --output-dir performance-plan --output json
```

The plan/receipt must disclose for each span:

- which requested dimensions the engine can execute natively;
- the exact engine parameters or deterministic post-processing used;
- which dimensions are advisory-only and therefore **not executed**;
- input manifest, sidecar, exact cue text and reference-audio hashes;
- seed, engine/version, output chunk hash and duration;
- any fallback, clamp or unsupported request.

This prevents the current failure mode in which a poetic direction exists in a
ledger but the model never receives it.

## P1 — local audition composer

Add a private, provider-neutral audition composition contract that can place
short variants behind neutral slates/chimes without changing their text. Useful
dimensions are `intensity`, `reference-window`, `phrase-grouping`, `pace` and
`pitch-shape`. It should reuse comparison receipts but identify voice-biometric
privacy and prohibit implied approval.

## P1 — performance continuity and QC

Add checks for:

- uniformly high intensity across an entire scene;
- missing contrast before or after a declared peak;
- performance spans that cross speaker or source boundaries;
- pause drift around protected hinges;
- output clipping/true peak and excessive loudness change;
- duration drift against captions and shots after a performance replacement.

Pitch/energy measurement may be evidence, but it must not claim to prove a human
emotion or culturally authentic performance. Human listening remains the gate.

## Acceptance fixture

Use sanitized Spanish placeholders, not BERTICA text or voices. The fixture
should prove:

1. one cue split into neutral setup, 0.95 explosive interruption and 0.82
   fear-driven warning;
2. one suspense cue with a protected pause before a short decisive action;
3. one dry comic button after the peak;
4. stale text hash and overlapping spans fail;
5. an engine unable to execute pitch shape reports `advisory-only` rather than
   pretending success;
6. receipt re-verification detects changed audio, sidecar or manifest;
7. existing manifests remain valid and no schema bump is required.

## BERTICA integration target

Once implemented, BERTICA will bind its emotional score to exact narration cues,
compile a local Chatterbox plan, render a short private audition, and obtain
separate Bertica/Herman findings before replacing the full scene performance.

## v0.2.16+ follow-up — scoped emotion versus cadence contour

BERTICA's subsequent local IndexTTS 2.5 experiments exposed a distinction that
`reel.voice-performance.v0.1` names but cannot yet verify or compile precisely:
an emotion category, a speaker's baseline register, and the direction of a pitch
contour are separate production facts. Applying `surprise` to an entire sentence
raised the narrator's global register. Scoping it to the decisive clause restored
an adult overall register, but the terminal phrase rose when the intended
boundary was falling.

The next additive contract should support, per exact hashed span:

- `emotion_scope`: `whole-span`, `onset`, `body`, or `terminal`;
- `baseline_register`: `speaker-reference`, `lower`, `level`, or `higher`;
- `pitch_contour`: `level`, `rising`, `falling`, `rise-fall`, or `fall-rise`;
- `terminal_boundary`: `open`, `suspended`, `decisive-fall`, or
  `question-rise`;
- optional relative contour targets expressed in semitones, never absolute
  gendered or age-coded pitch values;
- span-specific join/pause intent so a surprised action can return to a neutral
  adult terminal without post-render tempo manipulation.

Engine plans must distinguish native emotion conditioning from actual contour
control. If an adapter can execute an emotion vector but cannot guarantee a
falling boundary, it must mark `pitch_contour` and `terminal_boundary` as
`advisory-only`; it must not claim that surprise implies a rise-fall contour.
Post-render global pitch shifting and phrase time stretching must be separately
disclosed and may be prohibited by the sidecar.

Add a path-free prosody evidence receipt with per-span median F0, robust first /
middle / final F0 summaries, voiced-frame coverage, duration and detected trend.
These measurements verify whether a requested contour occurred; they do not
prove emotion, age, gender, authenticity or human approval.

Acceptance fixture: neutral adult-reference setup, localized surprise on a
short action span, and a separate decisive-fall terminal. Prove that global
emotion spillover and an accidentally rising terminal are detectable while the
fixture remains synthetic and contains no BERTICA text, voice, name or path.

Follow-up evidence from BERTICA packet 025 strengthens this requirement. A real
falling performance reference moved from roughly 215.5 Hz to 137.3 Hz, yet the
short synthesized terminal conditioned from it moved from roughly 162.8 Hz to
242.6 Hz. Treat an `emotion_audio_prompt` or equivalent style reference as
conditioning provenance, not proof that `pitch_contour` executed. Only measured
output evidence may report the resulting trend, and a mismatch must remain a
visible failed or advisory-only direction rather than silently passing.
