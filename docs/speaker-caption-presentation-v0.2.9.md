# Speaker-aware caption presentation in CLI v0.2.9

REEL v0.2.9 runs the v0.2.8 accessibility policy before every animatic render.
An inaccessible SRT exits before FFmpeg, MP4 publication, or artifact-report
publication. Existing captions can use documented threshold overrides, but a
non-default value requires `--caption-policy-note` and remains in lineage.

Speaker badges use the separate strict `reel.caption-presentation.v0.1`
sidecar. It maps every delivery SRT index to an existing narration-cue ID and
maps existing speaker IDs to explicit audience labels. Multiple delivery cues
may cite one source cue; REEL checks that each delivery interval remains inside
that conformed narration cue. It never derives a speaker from caption text,
filename, production ID, or formal display name.

```yaml
schema: reel.caption-presentation.v0.1
speakers:
  - speaker_id: guide-alpha
    audience_label: Guide One
cues:
  - { srt_index: 1, narration_cue_id: cue-a }
  - { srt_index: 2, narration_cue_id: cue-a }
```

```text
reel animatic-render manifest.yaml --asset-root assets --silent \
  --captions captions.srt --caption-presentation presentation.yaml \
  --caption-profile youtube-review \
  --speaker-label-policy first-entrance \
  --output review.mp4 --format json
```

Policies are `none`, `first-entrance`, `persistent`, and
`reintroduce-after-ms`. The last requires a positive
`--speaker-reintroduce-after-ms`. `youtube-review` requires 16:9,
`phone-review` requires 9:16, and `private-review` preserves the earlier caption
scale while adapting to either orientation.

Badges use a separately styled high-contrast box in the upper safe region.
Spoken captions remain in the lower safe region and the SRT bytes never change.
The artifact report records `reel.caption-lineage.v0.1`: the SRT and preflight
report hashes, thresholds and override note, profile, policy, sidecar hash,
computed presentation hash, deterministic pixel geometry, and label events.
`animatic-check` reconstructs and compares this lineage for v0.2.9+ artifacts.
Receipt generation therefore fails if the SRT, sidecar, thresholds, policy, or
event schedule has been tampered with; the path-free receipt itself exposes no
speaker or label.

The sanitized `manifests/fixtures/speaker-captions/` proof is exactly 42.155
seconds, has three synthetic speakers, eight narration cues, and eleven delivery
cues, and contains no BERTICA material. Real FFmpeg acceptance renders the
`none`, `first-entrance`, and `persistent` policies at 1280x720 and 720x1280.

This presentation evidence does not infer identity, consent, approval,
translation accuracy, or semantic agreement with audio. `reel.manifest.v0.2`
is unchanged and existing manifests require no migration.
