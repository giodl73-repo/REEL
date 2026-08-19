# OpenTimelineIO export (v0.2.40)

REEL v0.2.40 exports a validated production picture timeline as native
OpenTimelineIO JSON. REEL remains authoritative for the source manifest,
millisecond timing, shot identity, and asset status. The receiving editorial
system remains authoritative for media relinking, edit changes, transitions,
effects, tracks, and creative decisions.

## Export

`otio-export` accepts a `reel.manifest.v0.2` with `conformed` or `locked`
timing and writes a no-clobber `.otio` file:

```powershell
cargo run --bin reel -- otio-export manifest.yaml `
  --output-path timeline.otio --output json
```

The command runs REEL's complete production validation before export. Invalid
shot identity, scene links, missing timing, zero duration, gaps, overlap, or
mismatched scene/platform/export duration fail before an OTIO file is written.
Guide and untimed manifests are rejected because V1 targets stable editorial
handoff rather than provisional planning.

## Exact time

REEL production timing is millisecond-based and can contain durations that are
not exactly representable at a conventional video frame rate. V1 therefore
uses OTIO `RationalTime` values at rate `1000`:

```text
value 1005 at rate 1000 = 1.005 seconds
```

This is an interchange timebase, not a declared delivery frame rate. REEL does
not invent 24, 25, 29.97, or 30 fps or round the source timeline to frames.

## OTIO shape

The file contains:

- one `Timeline.1`;
- one `Stack.1`;
- one video `Track.1`;
- one ordered `Clip.2` per REEL shot;
- one `MissingReference.1` per clip;
- `TimeRange.1` and `RationalTime.1` source ranges;
- namespaced `metadata.reel` at timeline, track, clip, and media-reference
  levels.

Timeline metadata binds the exact source-manifest SHA-256, REEL manifest
version, work ID, timing status, and authority boundaries. Clip metadata
preserves stable shot and scene IDs, timeline start, duration, source-in time,
media kind, normalized asset status, whether an asset was declared, and a
SHA-256 binding for the authored transition intent.

## Deliberate offline-media boundary

V1 emits `MissingReference` for every clip, including clips whose REEL manifest
declares a local asset. This prevents workstation paths and private URLs from
entering portable output and prevents candidate media from becoming selected
through export. The editor or owner-controlled adapter must relink media
explicitly.

The file contains no visual prompts, action prose, source manuscript paths,
continuity notes, credentials, provider payloads, local media paths, or URLs.
Transition prose is also excluded; its hash preserves an exact binding without
turning free text into an OTIO transition claim.

## Deferred semantics

V1 does not emit:

- OTIO `Transition` objects, because REEL's current `transition_out` is
  authored intent without exact offsets or media handles;
- audio tracks, because narration, music, ambience, effects, fades, overlaps,
  and mix authority require a separate exact mapping;
- markers, effects, alternates, media embedding, OTIOZ, import, or round-trip
  mutation;
- AAF, EDL, FCP XML, Premiere, Resolve, or other adapter compatibility claims.

Export never selects media or grants creative, rights, publication, or release
approval.
