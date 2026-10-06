# Motioncraft queue comparison — simulated craft evaluation

Reviewer: current assistant applying the five core `.roles` checklists and
`scoring/RUBRIC.md`. No independent reviewers, human viewer, comprehension
study, creative selection or publishing approval participated. Scores are
bounded editorial judgments about this synthetic study, not automated truths
or evidence of model-provider performance.

## Evaluated artifacts and observations

The package `target/motioncraft-handoff-r1/package.json` has SHA-256
`864a4427d436a8ac999291879a6879fd949a4e8f0c87fc6c8734f39806f0bf0f`.
It contains both profiles' baseline/revised/reduced renders and corrected
captioned candidates (`review-comparison-v2`). Each profile's comparison
report confirms identical native audio bytes, shot spans, 15-second duration
and 450-frame count. Both clean hydrated reproductions also match all decoded
picture frames and indexed review samples.

Matched boards inspect frames 44, 149, 269 and 389. Baseline uses competent
static graphics; revised uses the same selected graphics with a bounded 1.00
to 1.08 camera landing and reading hold; reduced retains the baseline framing.
The 3-second first shot settles at frame 10 and holds from frame 11; the
4-second shots settle at local frame 14 and hold from local frame 15. Thus
each claim has over 2.6 seconds of stationary reading time. The camera makes
type and tokens 8% larger after settling without changing their hierarchy.

Local in-app browser playback used `tools/motioncraft_playback.html` copied
into the study parent directory. Observed normal landscape native playback
at 0.171 and 11.938 seconds; quarter-speed landscape playback at 0.028,
4.228 and 11.631 seconds; portrait muted playback at 0.224, 5.674 and 10.057
seconds; and native portrait playback from 0.187 through the reported 15.000
second end. The portrait review used a 360-pixel browser viewport, showing
the revised explanatory text, captions and diagram label simultaneously.
Desktop comparison used a 1280-pixel viewport plus native-sized extracted
frames. Normal landscape captioned playback was inspected as well.
These are sampled playback observations, not a claim that every frame was
visually inspected. Independent full-decode matching supplies the technical
frame/timing evidence.

Sound-on files contain original brief mathematical cues and intentional
dialogue/music silence. Native stem matching verifies their preservation.
No listening judgment or voice performance is claimed. The local comparison
player now exposes one audio focus to avoid three copies playing together.

## Findings and dispositions

| Role | Finding | Disposition |
|---|---|---|
| Story Director | Hook, growth, recovery and management form a coherent explanation; the same claims survive reduction. | Retain the original script and selected images. The diagram abstracts processing rather than depicting a worker action. |
| Animation Director | A brief landing introduces each concept, then the camera stops rather than drifting during reading. | Retain restrained 1.08 zoom, bounded easing and explicit holds. Patterned-pixel tests and the review evidence verify the execution. |
| Editor | Equal second/third/fourth shot lengths are deliberate reading time; the land/hold contrast gives the revised static sequence an additional punctuation. | Small Rhythm gain, not a major pacing transformation. Retain cut times and native narration clocks. |
| Sound Designer | Muted comprehension depends on authored text; sound is structural punctuation, with no narration to duplicate. | Captions reproduce study text, not a fictional spoken transcript. Identical native stems prevent a motion upgrade from moving the cues. Listening quality remains outside this evaluation. |
| Platform/Audience | Portrait headline and claim remain readable at phone width; small diagram process labels are secondary and gain only slightly from zoom. | Small Legibility gain. Do not infer viewer comprehension. Landscape is primarily a desktop/rotated-device treatment; a three-column comparison is not its native viewing size. |
| Platform/Audience | First landscape caption derivative collided with its lower process label. | Changes requested, corrected in v2 by reducing caption size/adjusting margin; inspected native caption frames and playback keep the label clear. First derivative retained as failure evidence. |
| Rights/Provenance | Original graphics/audio and an explicit locally hashed font avoid copied reference compositions and family media. | Internal review only; font bytes are not redistributed and creative selection remains pending. |

Retained disagreement: eight-percent camera motion is a modest benefit, and
some viewers may prefer baseline/reduced stillness. The gain applies to this
specific land/hold study and does not make zoom mandatory for future episodes.
On-screen captions are redundant with the explanatory headline by design in
the muted derivative; a narrated consumer episode should keep them complementary.

## REEL dimension scores

All dimensions are out of 25. The same questions and evidence scope apply to
each treatment. Scores include the corrected captions and technical render
proof; they do not treat their initial overlap as an accepted defect.

| Profile/treatment | Rhythm | Emotion | Execution | Legibility | Total |
|---|---:|---:|---:|---:|---:|
| Landscape baseline | 15 | 16 | 20 | 18 | 69 |
| Landscape revised | 17 | 16 | 20 | 19 | 72 |
| Portrait baseline | 15 | 16 | 20 | 19 | 70 |
| Portrait revised | 17 | 16 | 20 | 20 | 73 |

Rhythm increases by two for brief concept landings followed by genuinely
stationary reading intervals: observed quarter-speed playback and local
phase-boundary frames 10/11 and 14/15 support that judgment. Legibility
increases by one because the settled type/diagram are larger while keeping
the full claim and caption clear: matched frame 149 and the native caption
check frames provide the clearest comparison. Emotion remains unchanged
because the causal story and restrained tone are identical. Execution stays
unchanged because the additional motion introduces no observed clipping,
sample drift or review mismatch, while the captions are the same corrected
treatment in both variants.

The reduced-motion versions preserve baseline information, timing and
framing; no improvement score is required for them. The quality target is met
in this simulated evaluation: Rhythm/Legibility improve, Emotion/Execution do
not regress, and totals exceed 60. This closes the study's advisory craft
comparison, not the full Motioncraft goal. Actual CAIMITOS integration,
new/upgrade/rollback canary and final requirement audit remain open.
