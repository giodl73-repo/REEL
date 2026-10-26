# Scene delivery input provider review

Scope: exact media/evidence lookup through a borrowed provider; producer-applied
checklists, not independent approval. Source: REEL
ab14406bd1d5786f90bbb9f9d99af82dcd29b2c4 initially; rebased onto
4465d43f8ed808e7c4f9dc5f613b9a23627a3971 after the alignment reader merged.
No cache or runtime binding changes.

- Editor: job names and byte pins remain explicit; changed versions cannot be
  selected through a hidden latest lookup. Local/injected plans preserve native
  clocks, and real renders compare every decoded frame and PCM sample.
- Sound designer: D/M/E and audio-only rendering use the same provider during
  planning, decoding and independent checks. No resampling/mix policy changes.
- Rights and provenance: provider cache locations do not enter output receipts;
  media, timed overlays, derivation inputs, fonts and camera evidence use one
  provider contract. Authored local assembly controls retain their original checks.
  Technical access does not grant selection, consent or publication authority.

Resolved design finding: changing the existing public functions to generic inputs
could break downstream type inference. Their original signatures now delegate to
separate `*_with_inputs` entry points. The local-root provider retains its existing
semantics rather than gaining an unproven guard claim.

Trust boundary: injected providers must verify pins and retain effective byte/path
protection throughout their borrowed lifetime, including child processes. An API
cannot sandbox a provider that lies or mutates its guards through interior
mutability. Cooperative read/seek access is not a path-provider substitute.

Local verification so far: mapped-provider plan parity and pre-publication
missing/stale refusal pass. The real mapped-provider FFmpeg case passes decoded
pixel/PCM parity, full and audio-only rechecks and stale-binding refusal. CI
explicitly runs this external-tool case on both platforms. Actual Tig guard
rehearsal now passes locally: two cels share a cue; a cel-B-only Git revision
changes second-half pixels while first-half pixels and mixed PCM remain equal.
Held-source write/delete refuse, unrelated object creation succeeds, old-view
recheck passes after exact job-byte restoration, and reader counts return to zero.
Full final checks and current-head CI remain obligations until recorded.

Final local checks: scene-delivery target 25 passed/14 ignored, plus the new
ignored FFmpeg case executed explicitly and passed; root all-feature library
179 passed/eight ignored initially, then 183/eight after rebasing onto the merged
alignment reader. All-target/all-feature warnings-denied Clippy, formatting and
whitespace checks pass again on the rebased source. Current-head Windows/Linux
CI remains the merge obligation.

Disposition: pass with the documented provider trust boundary, subject to final
tests, Clippy and current-head platform CI. Full native family/producer parity,
job/source claims, custody, eviction and operational adoption remain open.
