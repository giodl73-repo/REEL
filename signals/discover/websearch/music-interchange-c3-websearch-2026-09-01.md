---
skill: discover-websearch
topic: music-interchange-c3
date: 2026-09-01
claims_checked: 4
confirmed: 4
---

# Public evidence for music-tool interchange C3

## Claims to ground

| # | Claim | Source of claim | Why it needs grounding |
|---|---|---|---|
| 1 | Existing source-separation users commonly receive named audio stem files. | REEL interoperability assumption | Intake must accept actual outputs rather than invent a new separator protocol. |
| 2 | Existing pitch-transcription users receive MIDI and may also receive note-event CSV, model arrays, or sonification audio. | REEL interoperability assumption | A MIDI-only intake would discard artifacts people already retain. |
| 3 | Existing feature-extraction workflows emit several tabular and structured annotation formats. | REEL interoperability assumption | The intake schema must separate file identity from semantic interpretation. |
| 4 | JAMS provides structured timed annotations plus provenance-oriented metadata. | Candidate interchange assumption | REEL should recognize JAMS without pretending to validate every external namespace itself. |

## Web evidence

### Claim 1 — separation outputs are named audio stems

- Query: `site:github.com/facebookresearch/demucs README separated tracks wav output`
  - Source: https://github.com/facebookresearch/demucs/blob/main/README.md
  - Direct quote: “four stereo wav files sampled at 44.1 kHz”
  - Relevance: the official Demucs README names drums, bass, other, and vocals WAV outputs.
- Query: `site:github.com/adefossez/demucs README output wav stems`
  - Source: https://github.com/facebookresearch/demucs/blob/main/docs/api.md
  - Direct quote: “for stem, source in sources.items()”
  - Relevance: the official API exposes a keyed stem collection and an explicit save step.
- Verdict: CONFIRMED

### Claim 2 — transcription outputs extend beyond MIDI

- Query: `site:github.com/spotify/basic-pitch README MIDI CSV note events`
  - Source: https://github.com/spotify/basic-pitch/blob/main/README.md
  - Direct quote: “save the predicted note events as a CSV file”
  - Relevance: the official CLI documents optional CSV, NPZ, and WAV products alongside MIDI.
- Query: `site:github.com/spotify/basic-pitch inference MIDI NOTE_EVENTS csv`
  - Source: https://github.com/spotify/basic-pitch/blob/main/basic_pitch/inference.py
  - Direct quote: `MIDI = "mid"` and `NOTE_EVENTS = "csv"`
  - Relevance: the implementation defines stable output-type extensions and CSV columns.
- Verdict: CONFIRMED

### Claim 3 — feature extraction has plural interchange formats

- Query: `site:vamp-plugins.org sonic annotator CSV output documentation`
  - Source: https://vamp-plugins.org/sonic-annotator/
  - Direct quote: “write the results in RDF or comma-separated text formats”
  - Relevance: the official site establishes multi-format feature output.
- Query: `site:github.com/sonic-visualiser/sonic-annotator CSV output`
  - Source: https://github.com/sonic-visualiser/sonic-annotator
  - Direct quote: “The following writers are currently supported.”
  - Relevance: the official README documents CSV, lab, RDF, provisional JAMS JSON, and MIDI writers.
- Verdict: CONFIRMED

### Claim 4 — JAMS is structured timed annotation JSON

- Query: `site:jams.readthedocs.io JAMS file format JSON annotations`
  - Source: https://jams.readthedocs.io/
  - Direct quote: “A formal JSON schema for generic annotations”
  - Relevance: the official documentation describes typed namespaces and validation.
- Query: `site:jams.readthedocs.io JAMS structure time duration confidence`
  - Source: https://jams.readthedocs.io/en/stable/jams_structure.html
  - Direct quote: “time, duration, value, confidence”
  - Relevance: the official structure maps directly to REEL’s need for bounded observation evidence.
- Verdict: CONFIRMED

## Findings

| # | Finding | Verdict | Source |
|---|---|---|---|
| 1 | Four-stem WAV output is an established separator workflow. | CONFIRMED | Demucs README |
| 2 | Stem names carry provisional semantic roles such as vocals or drums. | CONFIRMED | Demucs README |
| 3 | Separation output may be int16, int24, or float32 WAV. | CONFIRMED | Demucs README |
| 4 | Separator APIs expose stems before users choose their storage container. | CONFIRMED | Demucs API |
| 5 | Transcription’s default durable interchange includes MIDI. | CONFIRMED | Basic Pitch implementation |
| 6 | Note-event CSV is a first-class optional transcription output. | CONFIRMED | Basic Pitch README |
| 7 | Note CSV contains start, end, MIDI pitch, velocity, and pitch-bend data. | CONFIRMED | Basic Pitch implementation |
| 8 | Raw transcription model output may be retained as NPZ. | CONFIRMED | Basic Pitch README |
| 9 | MIDI sonification may be retained as a WAV derivative. | CONFIRMED | Basic Pitch README |
| 10 | Feature extractors can emit CSV. | CONFIRMED | Sonic Annotator README |
| 11 | Feature extractors can emit tabular lab files. | CONFIRMED | Sonic Annotator README |
| 12 | Feature extractors can emit RDF with source-signal references. | CONFIRMED | Sonic Annotator README |
| 13 | Feature extractors can emit MIDI notes. | CONFIRMED | Sonic Annotator README |
| 14 | JAMS supports multiple annotation namespaces in one JSON document. | CONFIRMED | JAMS docs |
| 15 | JAMS observations explicitly carry time, duration, value, and confidence. | CONFIRMED | JAMS structure docs |
| 16 | JAMS metadata can describe program or human annotators and collection rules. | CONFIRMED | JAMS structure docs |

Summary: 4 of 4 claims confirmed; 0 contradicted; 0 unconfirmed.

## Ungrounded claims

No ungrounded claims. Compatibility with any particular proprietary desktop
application remains outside this evidence and requires sanitized real-user
fixtures or direct operator confirmation.

## Amend

1. Treat container/media format separately from semantic purpose: WAV may be a
   stem or a sonification, and MIDI may be a transcription or a feature writer.
2. Preserve original external bytes plus producer version, parameters, model,
   license, and network policy before normalization.
3. Recognize JAMS structure conservatively; do not claim complete namespace
   validation without the upstream schemas.
