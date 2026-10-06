---
stage: handoff
created: 2026-10-06
updated: 2026-10-06
sources: []
---
# Native poem and semantic picture transform support

Consumer request: a continuous selected native poem reading has several canonical
display lines; a physical carriage jolt spans cuts between distinct pictures.

Implementation preserves canonical source IDs and adds optional audio_cue_id to
source/display lines. Exact source ownership and canonical order are checked;
native semantic markers own display entrances. Default one-cue-per-line behavior
is unchanged.

timed-picture-transform is a typed delivery mode chosen explicitly per VFX
binding in the scene delivery profile. A selected hash-bound JSON recipe owns
bounded zoom/translation. Existing semantic attachments own language-local
windows. The renderer derives a post-composition camera from the recipe, retains
pre-camera picture, and checks unchanged sampled outside frames, visible active
frames and exact frame counts. Multiple recipes or an independently authored
camera conflict and fail. Alpha-video overlays remain separate.

Validation executed: cargo test --test scene_delivery --test
template_presentation --quiet: 15 scene-delivery tests passed, 2 previously ignored;
12 presentation tests passed. New synthetic fixture crosses two distinct cels,
checks hash tampering and invalid bounds. Native consumer poem and chapter compile
commands pass in both languages. No claim of actual listening or creative approval.

Read-only specialist /root/e7_reel_build_path inspected the existing engine,
proposed source-ID/audio-ID separation and a semantic transform mode; producer
owns all edits. Requested gpt-6-sol/high; runtime identity unavailable. A later
source-window followup failed model capacity and supplies no review approval.

Review lenses: editing requires native cuts and outside-span identity; sound
requires separate Sonic and VFX; story requires an effect explicitly motivated by
consumer source rather than a decorative global camera; rights retain upstream
scope. These are producer checklist findings, not opinions of real principals.

Remaining: consumer context audition; true instrumental score selection remains
an upstream creative decision. No child media or private manuscript is stored here.

Integration recheck: origin/main ac29636 (authored motion compiler) arrived during
this work. Merge preserves the upstream byte-based plan hash, motioncraft and
caption layout behavior while allowing recipe-derived camera state. Release
test run passes 18 scene-delivery tests and 12 presentation tests; 4 pre-existing
scene-delivery tests remain ignored. Existing E7 private compilation used the
a0a1d38 runtime snapshot; runtime executable SHA values are retained by its
consumer rather than assumed equal to a later main build.
