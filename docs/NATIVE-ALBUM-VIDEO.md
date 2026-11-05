# Native album video and bilingual reading pages

`reel-album-video plan REQUEST`, `render REQUEST NEW_DIRECTORY`, and
`check REQUEST DIRECTORY` project a retained complete MP3 album into a private
H.264/MP3 MP4. Audio packets are copied in order, not re-encoded. This generic
Rust producer starts no service, discovers no project files and publishes nothing.
Tig/managed callers resolve the authoritative album and supply exact materialized
inputs. A normal job can use `render request.json render`.

The `reel.album-video.v1` request declares pinned FFmpeg/FFprobe, audio,
chapter metadata and album receipt; fonts/families; labels/style; optional reading
plan; private permissions; and input/output/time limits. Native album receipts
also require their exact original album request to recover its bound title.
The existing `reel.local-song-album-master-check.v1` ledger is a read-only input
adapter, never a command to run a historical Python producer. Source paths may be
absolute or normal relative paths beneath the request directory. Traversal and
relative symlink escapes refuse. Tool paths are explicit absolute enrollments.

Without a reading plan, the profile is 1280×720 with restrained track labels.
With a plan, the phone profile is 1920×1080, two columns and seven visual rows.
Both use five frames per second over the full album clock. Title, footer, byline,
language headings, instrumental/wordless descriptions, families and palette are
caller data. There are no project or creator credits hardcoded into the renderer.
The reading profile retains its stepped frame and central fan ornament. Track
titles use the body family with `track_title_bold`; headers use the heading
family/weight and page numbers its regular face. Declare every used font weight;
the planner refuses missing faces rather than requesting synthetic bold.

Reading input may use `reel.album-reading.v1` or the retained
`reel.album-bilingual-reading.v1` format. The exact source album receipt is bound,
and every ordered track ID must agree. Pages have integer album-frame bounds.
Editorial holds remain distinct from declared performed-phrase windows; this
validates supplied clocks, not speech recognition or translation correctness.
Performed pages require one phrase per timed pair, with canonical-context-only
rows explicitly distinguished. Overlaps, out-of-track phases and missed
windows refuse; declared blank intervals remain blank. It is not word highlighting.

Source-approved `display_lines` may change wrapping while preserving every source
word. Missing translations, ASS controls, excessive rows, missing glyphs and text
that exceeds a column refuse. The retained 58/64-pixel body settings and optional
instrumental-divider clearing are supported. New-format pairs use `left`/`right`;
the legacy adapter reads `es`/`en`. Simple labels neutralize ASS syntax characters;
reading wording itself is never rewritten. Pinned font family/weight and glyph
metrics are checked. This declares the supplied fonts, not a complete system-font
provider/environment attestation.

The packet contains `master.mp4`, generated `labels.ass`, a compact `layout.json`
ledger, and `receipt.json`. Only a new directory can render. Failed bounded
partials stay available for recovery. Copied font scratch is removed after a
successful encode/verification; cache bytes and source media are never removed.

Checks include output hashes, independently reconstructed layout data, codec,
canvas/fps, chapter titles/clocks, identical audio-packet counts and ordered payload
hashes, identical complete decoded PCM on the exact album sample clock, full decode and
the total output budget. Metadata/text changes invalidate the old packet. This
does not claim an independent pixel-by-pixel source comparison for arbitrary
outputs, musical acceptance, human picture review, selection or delivery.

The audio stream must declare exactly the album's sample count at 48 kHz and
start at zero. Source and movie must preserve the same initial encoder skip.
The mux uses a 48-kHz movie timebase and explicit sample-duration packet metadata;
the final packet remains present with its payload unchanged. This preserves
sub-millisecond endpoints even when the final five-fps picture extends longer.
The checker derives final discard padding from the unchanged MPEG-1 Layer III
packet count, initial skip and exact source PCM length; it must be less than one
1,152-sample packet. Some older MOV demuxers expose those final padding samples
despite a correct edit-list duration. The decoded movie may expose either none
or exactly that proven padding, while every album sample must still match.
Both the source padding and decoder-exposed count are reported. No arbitrary
tail tolerance, audio re-encoding or shortening of the declared album is allowed.

The real synthetic fixture covers editorial and performed pages, explicit fonts,
Unicode, two distinct columns/four wrapped rows, visible phrase entrances and
blank intervals, an instrumental hold, exact MP3 packet/PCM preservation, chapter
round trips, stale-plan refusal and tamper refusal. Set `REEL_TEST_FFMPEG`,
`REEL_TEST_FFPROBE`, `REEL_TEST_FONT`, and `REEL_TEST_FONT_FAMILY` explicitly before
running `cargo test --lib music_album::video::tests -- --nocapture`. Hosted CI
sets them on both platforms; an unset local media fixture is not qualification.

Producer-applied role checks use music reconstruction and sound for complete
audio/clock identity, editing for order and holds, platform/audience for readable
geometry, lyric adaptation for preserved bilingual wording and declared timing,
and rights/provenance for private scope and separate human authority. Technical
passes do not authorize publishing or establish independent review.
