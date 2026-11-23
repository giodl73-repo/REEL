# Native prefix compositions

`compose-parts composition.json --asset-root ROOT --output-dir NEW-DIR`
consumes `reel.programme-part-composition.v1`. This first bounded version has
exactly two complete, nonrecursive v2 source sets: one single-segment prefix,
then the complete body. It accepts no caller offsets, trims, subsets or crossfades.
Each source declares `id`, a relative `asset_root`, a hash-bound `set` FileRef
(relative to ROOT), and a staged `producer` executable FileRef (relative to ROOT).
The set's original manifest/proof/media paths stay relative to its source root.
`order` lists both source IDs once. `manifest` selects the complete new target.
All files must be declared in the closed native asset-job scope.

The current producer and the explicitly qualified historical producer
`11da1731ddff8ab6df0cf93ad04596a950204b1dcd41699ff11e5b5b43e3b227`
are supported here. Ordinary `finalize-parts` keeps its current-executable gate.
Each original set passes the original complete v2 proof/media checks before any
new placement is derived. The target must preserve every selected segment and
must select a valid ordered v2 master template and episode control closure.
Source output clocks, compact profiles and full stream signatures must match.
Block offsets must be integral sample boundaries. No original proof is rewritten.

The v3 receipt wraps a composition-specific finalization record and separate
source/proof/producer identities plus original and effective segment placements.
It verifies every compact decoded picture, full PCM, output clocks and ordered
coded H264 packet payload hashes. The finalizer copies H264 and performs one
continuous MP3 encode from the concatenated original PCM24 parts. It does not
reuse old MP3 packets or claim unseen retained lossless bytes were freshly read.
The final output publishes only after verification into a new directory.

The typed caption consumer accepts the v3 receipt and its effective placement
clock. Supply a zero-cue SRT/words-array track for the first presentation segment,
then the unchanged original caption tracks. Text and local timings retain their
existing source hashes; only programme offsets change. Human approval, rights,
listening, typography and complete programme acceptance remain separate gates.

Browser compositions may opt into a `lossless_master` object in the existing
manifest. Defaults remain H264 CRF18 YUV420/AAC review. The opt-in output is
FFV1 YUV444 with stereo PCM24 in Matroska. Declare `sample_rate`, full decoded
`source_samples`, `start_sample`, `end_sample`, five strictly ordered source
sample `anchors` (pickup, three entrances, phrase endpoint), and exact
`selected_pcm_sha256`/`envelope_pcm_sha256`. The pickup equals the window start;
the endpoint equals its end. The window must equal the exact picture duration.
The original stereo PCM WAV is normalized once to PCM24 at its declared rate;
no resampling, padding or MP3/AAC source is accepted. Optional `fade_in_samples`
and `fade_out_samples` apply integer linear gains: entrance sample i gets i/N,
exit sample i gets (count-1-i)/N, with signed division toward zero. Zero values
preserve all selected samples. Selected and enveloped PCM hashes are checked.

The render compares the complete captured PNG sequence's YUV444 decode against
the FFV1 output, and compares output PCM against the selected envelope. The
checker rechecks these decoded identities and source window. Animation remains
explicit `reelframe` driven DOM/SVG; autonomous SMIL is not synchronized. Bind
fonts and every visual source through the existing hashed asset list. Musical
note correctness and visual approval are producer/listening decisions.

Existing asset-job admission limits and lifecycle ownership are unchanged;
account for staged tools/inputs, source normalization/captured frames, complete
continuous PCM scratch, faststart/copy overlap, and twice retained output caps.
No admission increase, media discovery, hydration, registry write or production
adoption is performed by either command.

Each original source's catalog, ordered master template, episode and scoped
bindings must also be staged under its declared `asset_root`; they are freshly
hashed and checked against the original manifest. The prefix is a presentation.

`lossless_master.source_clock` is a hashed asset selecting the existing
`reel.presentation-source-clock.v1` contract. Its source recording hash, rate,
full sample count and evidence FileRef must match the original recording.
Declare the clock and its original evidence in `assets`. `anchor_ids` selects
five unique named anchors, matching `anchors` exactly. Evidence remains
producer-applied timing evidence; it confers no human or musical approval.

Each `reelframe` event includes `detail.sourceClock` for master mode, with
`sampleRate`, `startSample`, `endSample`, original `anchors`, and `anchorFrames`.
An anchor maps to the nearest frame using
`floor(((sample-startSample)*fps + sampleRate/2)/sampleRate)`.
Editable source should drive its three entrances from `anchorFrames[1..3]`;
`anchorFrames[0]` is pickup and `[4]` the exclusive phrase endpoint. Review mode
receives `sourceClock: null` and retains its existing frame/time/FPS fields.
