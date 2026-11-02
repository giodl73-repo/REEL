# Native Tig browser compositions

Status: source planning and native streaming renderer implemented; managed job
integration is a candidate undergoing synthetic acceptance. Private project
acceptance and lane activation remain open. Entry source
`52afd03b7558713a87cc470918f94c7ecd0608d2`.

The BROWSER-01 candidate now implements `tig_browser::{lower, requirements,
freeze_context, restore_context, recipe}`. These are metadata/source-planning
ports only. The renderer now supplies guarded resource delivery, bounded native
streaming and an explicit producer declaration. Managed output correspondence
and actual project acceptance remain separate. A source recipe is
not a completed output or permission to reuse it. The eventual producer must
also bind actual rendering flags and encode parameters into output identity.

An explicit metadata preview is available in the candidate CLI:

```text
reel-tig-scene --profile PRIVATE_SOURCE_PROFILE plan-browser PRIVATE_REQUEST
```

It prints bounded counts and opaque pins, opens no primary media, starts no
browser and creates no output directory. Errors omit raw resource paths and IDs.
The source profile uses the existing exact-commit/definition/byte-provider
admission; it neither discovers nor initializes a cache.

The request has schema `reel.tig-browser-source.experimental-1`, `root_id`,
`root_definition`, `primary_definitions`, optional exact `private_review`, and
`bounds` (`max_inputs`, `max_input_bytes`, `max_frames`, `max_canvas_pixels`).
The admitted root's authored data has this shape:

```json
{
  "document": {
    "source": "opening.svg",
    "audio": "audio/music.wav",
    "inputs": {
      "opening.svg": {"id": "assets/source-svg"},
      "pictures/a.png": {"id": "assets/picture-a"},
      "audio/music.wav": {"id": "assets/music"}
    },
    "width": 1280,
    "height": 720,
    "fps": 24,
    "frame_count": 2880
  }
}
```

`audio` may be absent/null; optional `dynamic_regions` retains the existing
pixel-region meaning. Inputs accept ID-only references: a second authored hash
or pin refuses. Primary payloads follow the exact admitted selected/named-stream
policy. Case aliases, prefix collisions, unsafe paths, unbuilt or unadmitted
inputs and exceeded shared bounds refuse. Counts distinguish logical aliases
from unique objects; no physical-copy or held-capacity claim follows.

Reuse `browser_composition`'s existing persistent document, explicit frame clock,
capture and lineage behavior. Tig owns exact metadata/byte resolution, while REEL
owns SVG/browser/media semantics. Do not convert an existing SVG into a different
still sequence merely to fit another producer, or treat an old movie as a primary.

## Authoritative authoring and lowering

### Managed candidate entry point

The existing `reel-tig-job capture-context PROFILE NEW_FRAGMENT` accepts
`reel.tig-browser-capture-profile.experimental-1`. It freezes and reads back the
browser context into explicitly configured existing stores, without a preparation
cache, primary media reads, source adoption or rendering.

Complete the fragment into the ordinary
`reel.tig-native-job-profile.experimental-1` and invoke
`reel-tig-job run PROFILE`. Set `worker.context_mode` to `browser`, retain the
frozen source/context/request, and enroll `worker.runtime` normally. Add
`worker.browser` containing `program` (absolute executable path/hash/size),
`clean_picture`, and the native renderer's explicit `limits`. The output map is
exactly `master.mkv` at `limits.max_output_bytes` and `receipt.json` at 65,536
bytes. Browser jobs currently require these explicit bounds; scene output-budget
inference does not apply. This native master is FFV1/PCM, not a delivery encode.

The existing coordinator owns source claims, capacity, worker containment,
retention and readback. Browser jobs add no cache namespace or global lock.
Existing `record_outputs` and `git_handoff` produce retained, unapproved candidate
records and an expected-base Git proposal; rendering does not change a branch
or select the result. Previous streams stay in the candidate record and previous
records stay in Git history. Windows containment is required. Neither legacy
checkout is needed after context capture; executable and byte-store enrollments
must remain available. See the explicitly run synthetic
`managed_browser_cli_renders_retains_and_proposes_with_source_offline` fixture.
The archived primary byte enrollment must require `prevent-mutation`;
cooperative metadata planning alone is insufficient for execution and refuses
before source adoption or a worker starts.

One source instance declares geometry, frame clock, the logical SVG path, optional
audio path and a bounded map from logical resource paths to primary Tig IDs. The
map is the authoring authority; leaf records carry their independently versioned
byte selections. Editing one picture must change its leaf and the resolved recipe,
without requiring a second hash edit in the parent. Original browser-composition
manifests remain exact provenance, rather than a competing mutable asset list.

Lower the captured IDs into the existing `reel.browser-composition.v0.1` meaning.
Preserve source-relative resource paths and exact SVG content. All consumed
definitions, stream roles, eligibility and resource bounds must be admitted before
execution. Reuse existing private-review admission for exact source revisions;
technical preparation neither selects a draft nor grants creative approval.

Freeze controls, resolved primary objects, interpretation documents, producer
parameters and executable identities through the existing native context/job ports.
Recipe identity must track consumed changes while keeping annotations, source
checkpoint provenance and publication observations separate. Restored execution
must work with the original Git checkout and loose source files unavailable.

## Execution boundary and storage

The current legacy browser path copies resources, saves every frame, defaults to
system temporary directories and routes Windows FFmpeg through WSL. Wrapping that
CLI unchanged would not meet native job/storage requirements. Keep the legacy
entry point available; the new managed route needs these explicit boundaries:

1. Pin browser, native FFmpeg and ffprobe through admitted host/runtime bindings.
   Missing or changed executables refuse. No font discovery, browser installation,
   downloads, daemon or cache initialization follows from reading a project.
2. Resolve resources only through the frozen, guarded primary map. Prefer an
   isolated virtual document origin with CDP request interception: fulfill exact
   admitted paths from verified bytes and reject other origins, paths, redirects
   and schemes. Do not expose a cache directory or a filesystem root to the page.
   Refactor the existing CDP loop to handle resource events explicitly; silently
   discarding asynchronous requests would deadlock a document load.
3. Keep one bounded captured frame in the pipeline and feed admitted native
   encoders with backpressure. Do not accumulate a directory of PNG frames or
   another full copy of the resource closure. Frame/output buffers, decoded canvas,
   requests, metadata, browser profile, stderr and in-flight encoders all need
   declared bounds. Count aliases separately from unique source objects.
4. Use existing scoped job/capacity admission for every actual write, with caller
   selected existing storage roots. Any unavoidable materialization needs an
   explicit peak allocation allowance and owned cleanup. Zero-copy source delivery
   does not imply zero browser memory, profile or output allocation.
5. Hold source protection through consumption; guard loss, cancellation, browser
   exit, encoder failure and output-budget exhaustion stop owned children and
   refuse publication. Preserve existing outputs and foreign partials. No shared
   cache/render lock or automatic disposal of unassociated files is permitted.
6. Native master and delivery encoding are separate declared contracts. The legacy
   default is lossy H.264/yuv420p plus AAC, which cannot establish a lossless master
   correspondence. A shared frame capture may feed independently bounded encoders;
   each output still declares its original primary closure and encode parameters.

Browser/OS resource enforcement and descendant-process termination capabilities
must be explicit. Estimates, static SVG scanning and a free-space observation
cannot be reported as hard containment. A private producer remains unready where
the admitted worker cannot enforce its required execution bounds.

## Implementation sequence and verification

| Work package | Concrete exit |
| --- | --- |
| BROWSER-01: native source and frozen plan | Typed request/definition admission, one authoritative primary map, lowering/recipe/context replay, exact role/eligibility/budget refusals and leaf-only edit invalidation; metadata tests open no media or browser. |
| BROWSER-02: bounded renderer seam | Reuse clock/capture semantics with guarded virtual resources, bounded frame streaming, admitted native encoders, source-loss/failure cleanup and explicit runtime/resource enrollment. |
| BROWSER-03: managed producer and adoption | Existing capture/claim/publication/Git handoff, original-source-offline replay, retained current/prior lookup and independent full frame/sample/clock comparison; then separate lane activation/rollback. |

Entry/exit evidence stays separate: L0 formatting/strict Clippy/source checks;
L1 synthetic metadata and injected fault fixtures; L2 explicit tiny browser/native
encoder runs before a separately admitted private replay. Exact source, tool
bytes, budgets and outcomes are pinned. No rendered candidate inherits approval.

Required scenarios include:

- Two language documents share pictures/fonts/audio but keep independent SVGs.
- One picture changes by stable ID; the parent stays unchanged and the recipe
  invalidates. Frozen old metadata still resolves the previous picture.
- Output/control definitions cannot masquerade as primary images or audio.
- Missing/corrupt resources, wrong fonts and changed executables refuse before
  expensive execution, without fallback to a working file.
- Duplicate/case-alias/unsafe paths, foreign origins and oversized request bodies
  refuse; a dynamically constructed URL cannot escape the admitted map.
- Multiple input aliases share one verified object but retain distinct logical
  paths; no per-alias physical cache copy is claimed or created.
- Frame, request, output and allocation limits fail at their exact boundaries;
  a rejected frame/encoder cannot become a completed output.
- Guard loss, cancellation and a failed consumer kill owned processes and settle
  only owned admissions. Concurrent unrelated writers remain available.
- Native and delivery outputs use declared clocks, codec/audio rules and budgets;
  a lossy preview cannot satisfy a native master comparison.
- Original Git/control paths are unavailable during frozen replay; restoring a
  previous version needs no regeneration or new approval.
- Technical equality preserves the original approval subject; disagreement stays
  explicit rather than relaxing comparison or selecting a replacement.

## Producer review and remaining risks

Identity/Git review requires exact frozen inputs and leaf-only revisions. Storage/
recovery review rejects duplicated frame spools, unadmitted profile growth and
foreign cleanup. Rights/privacy review requires local guarded inputs, bounded
diagnostics and no external resource egress. Editing review requires preserving
the declared SVG timing and geometry, not an inferred reconstruction. These are
producer-applied checklists, not independent or human approval. BROWSER-01 has
[local implementation verification](reviews/2026-11-01-tig-browser-source-planning.md).
Managed integration qualification, private acceptance and runtime adoption
remain open; renderer implementation is present.

## Streaming capture interface

`tig_browser::capture` supplies a second candidate integration seam. It accepts
an already admitted CDP transport, guarded exact resources, explicit limits and
a synchronous frame consumer. It neither launches a browser/encoder nor creates
an output, profile, cache or staging directory. The actual runtime and managed
producer remain required; this seam is not BROWSER-02/03 completion.

The resource adapter borrows the held input provider. Each request maps to one
canonical URL under a virtual origin, with exact length/hash verification and
authority checks around reading. Unknown origins/paths, query aliases, bodies,
ranges and redirects refuse rather than falling back to network or working files.
The CDP dispatcher services paused requests while awaiting other responses and
checks every fulfillment acknowledgement, including reordered responses.

Request counts, cumulative served bytes, individual resources, inbound/outbound
messages, frame bytes, geometry and elapsed time have explicit bounds. The local
WebSocket adapter uses bounded messages and nonblocking reads/writes with an
absolute deadline; trickling a partial frame cannot restart that deadline. A
document resource-policy violation, authentication request or detachment refuses.
The resource policy is defense in depth, not a browser/OS network sandbox.

The shared legacy frame clock is reused. Every screenshot is decoded under
geometry/allocation limits before consumption; dynamic-region checks retain their
existing meaning. There is one synchronous frame in flight and an incremental
sequence digest, with no per-frame files or growing lineage list. Consumer drain
and final source-authority checks must pass before a capture summary exists.
The decoder allocation limit is a library limit, not an OS memory quota.

The caller must still provide protected source/program lifetimes, admitted peak
storage/process limits, bounded encoder writes, cancellation and process-tree
reaping. Errors return no completed capture; they do not independently terminate
an externally owned browser or consumer. There is no automatic output publication,
reuse, approval, retention or eviction authority. Native output recipes must bind
this implementation and all actual browser/encoder options before adoption.
