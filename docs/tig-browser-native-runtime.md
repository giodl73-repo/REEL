# Native browser worker integration

Candidate implementation following [browser source/capture planning](tig-browser-composition.md).
`tig_browser::render::native` is a low-level trusted Windows worker operation,
not a replacement for managed capture, source/job enrollment, capacity admission,
retention, record proposals or lane adoption. No production binding changes here.

## Inputs and native output

The caller supplies a resolved source plan, held input provider, bound native
media programs, separately bound browser, existing admitted workspace, new output
path, exact execution limits and source-authority callback. There is no ambient
program lookup, asset discovery, cache initialization, hydration or daemon.
The workspace and durable claims must be admitted by the outer job first.

The initial output contract is FFV1/yuv444p, square pixels and the declared
constant frame clock in Matroska. An optional WAV is explicitly resampled to
48kHz stereo, padded/trimmed to the source duration and encoded as PCM24. Total
duration must correspond to an integral sample count. Delivery codecs remain a
separate contract; a lossy preview cannot satisfy this native master contract.

The producer declaration binds actual protected runtime/program identities,
clean-picture choice, fixed native encoding contract and implementation pins.
It is separate from process/storage admission limits and from creative approval.
The managed integration must use this actual declaration in the recipe instead
of attributing the render to an exploratory source recipe or a historical tool.

## Allocation and child lifetime

The browser receives a fresh owned runtime directory containing its user profile,
working directory and TEMP/TMP. Exact resources are served through CDP; source
media and per-frame PNGs are never copied into that directory. Frames pass one
at a time to the native encoder over a bounded loopback connection. The caller's
input/program guards outlive capture, encoder drain, checks and child teardown.

Quiet children reuse the existing atomic Windows Job Object assignment. Each
tree has explicit aggregate committed-memory and process-count limits, a finite
deadline and cancellation checks. Only the declared standard handles are
inherited. Stdout/stderr go to NUL, avoiding another unbounded log. A successful
return requires stopping remaining owned descendants; error/drop paths cancel
and join owned child controllers. Unrelated processes are never terminated.

Workspace bytes/depth/entry counts are bounded observations during execution,
not an OS filesystem quota or a byte-exact peak-allocation guarantee. The outer
runner still owns capacity reservation and workspace accounting. Browser flags,
CDP interception and content policy are not an OS network sandbox or a complete
environment declaration. Platform DLLs, fonts and any additional interpretation
dependencies need explicit enrollment where required by the consuming project.

An observed child directory may disappear before recursive enumeration as
Chromium retires its own temporary files. Only that child `NotFound` case is
treated as absent. A missing workspace root, other I/O errors, reparse entries
and depth, entry or byte overruns still refuse; a failed observer stays failed.

The protocol thread samples workspace usage after at least 250 ms since its
previous completed scan, with forced scans at admission and before publication.
It does not recursively scan the browser profile on every CDP message or poll.
Source-authority checks still run on every guarded protocol check; child
controllers retain their independent workspace checks. A failed usage observation
stays failed even if the offending file disappears. These observations can detect
an exceeded budget but cannot prevent transient allocation between samples.

## Checked handoff and failure

Discovery and WebSocket handshake operate only on the owned profile's observed
loopback endpoint, with finite byte/time bounds. HTTP response length governs
discovery completion; an open connection cannot require waiting for EOF.
Wire messages accept CDP fractional timestamps while retaining duplicate-key
rejection; Tig identity metadata keeps its stricter numeric rules. Shutdown uses
an integer command ID within Chromium's signed 32-bit range and checks the reply
or actual process exit. The browser's non-rendered tab-icon request is explicitly
denied as an auxiliary request; Image/Document requests get no such exemption.
Equivalent browser URL escapes resolve once to the exact declared UTF-8 logical
path. Queries, fragments, encoded separators, traversal and unknown paths refuse;
URL normalization never searches the filesystem or another provider.

After capture, the encoder drains and the browser closes. The native output must
pass exact stream/codec/pixel-format/geometry/frame-count checks. Optional audio
is fully decoded through a bounded stream: its complete PCM hash and exact sample
count are checked without another media file. Source authority, workspace usage
and output-byte limits are checked before a no-clobber move exposes the result.
Existing outputs and unrelated files stay intact. No result becomes selected,
approved, retained or delivered automatically.

Synthetic actual-browser/encoder fixtures are distinct from private acceptance.
Managed frozen-source replay, original/current retained-reference comparison,
durable source/job handoff, output registration and normal lane entry/rollback
remain required before CAMINOS/Melena production adoption. Historical evidence
and its original runtime routing remain unchanged.

## Fresh primary-reference comparison

`tig_browser_reference::compare_master_to_primaries` explicitly renders the
current prepared primaries into caller-admitted temporary space, then uses the
existing full SDR picture/audio/clock comparison against a separately protected
master. Incompatible declared stream shapes refuse before starting a browser.
The expected movie is held and rehashed before comparison; success requires
exact equality and checked cleanup of the temporary reference.

The opaque result has no deserializer and cannot be created from a status or
retained self-comparison report. It describes a fresh comparison only. Its caller
must retain primary/output/runtime guards, supply source-authority checks and
reserve the reference render's full workspace/output peak through the scheduler.
This operation does not create a cache, adopt a source, select/publish a master or
authorize eviction. The managed presentation adapter must bind the result to its
current source ownership and exact episode occurrence before assembly can use it.

The candidate `tig_presentation::jobs::browser` adapter now supplies that ownership
layer through the existing job-context and per-source fence protocol. Metadata
freezing enrolls every declared primary/runtime dependency and the exact output
record; the selected master is a composition dependency, never a primary input.
Fresh comparison is required before the adapter creates the same sealed
`VerifiedPresentation` consumed by episode assembly. Retained self-comparison
reports and selection labels cannot construct that proof.

Episode presentation requests select their interpreter by schema. Existing still
requests keep their shape; a browser request uses the normal browser-source schema
and a sibling `browser` admission with explicit `program`, `limits` and
`clean_picture`. The output record must point to that source through
`authored.presentation.id` and identify its language/episode occurrence. Browser
reference workspace bounds must fit the episode's admitted reference-staging
allowance. No native browser profile or source code is copied into an asset repo.

Capture failures retain their original error chain and identify the control
command and send/receive direction, picture/frame-clock index, or consumer
write/finalization stage. Native encoder failures distinguish socket writes,
input half-close and child exit. Context includes no command parameters, source
bytes or picture payloads. A connection reset still fails the operation without
retry; stage context does not establish why the browser or encoder disconnected.
