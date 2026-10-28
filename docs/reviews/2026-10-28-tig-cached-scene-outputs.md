# Protected cached native scene outputs

`tig_scene_outputs::Bundle` opens an explicitly pinned native delivery receipt
through Tig's existing-layout reader and treats its output list as the exact
output manifest. It holds verified, mutation-preventing readers for the receipt
and all output objects, deduplicated by hash. No cache setup, copying, discovery,
daemon, download or implicit latest lookup occurs. The caller admits the receipt
pin and byte profile and keeps the bundle alive throughout consumption.

Receipt JSON is bounded to 4 MiB and rejects duplicate keys. Output logical names,
matching paths, hashes, consistent sizes and reader/byte budgets are validated
before opening payloads. An unbound or changed FileRef refuses. Only Windows
mutation-preventing path protection is admitted; cooperative locking cannot
silently stand in for it. Errors drop all partially acquired readers.

The bundle implements the existing `InputFiles` interface and feeds
`scene_delivery::check_with_output_files`. That full checker still derives the
current job's output contract and verifies every media/evidence output. Opening a
bundle alone proves bytes, not recipe currency, producer trust, approval, retained
custody or safe deletion. `objects()` exposes the complete receipt/output closure
for the caller's retention adapter without classifying outputs as primary inputs.

The isolated generated-media fixture publishes a complete native delivery to its
temporary CAS, removes the old render directory from lookup, and independently
checks the cached delivery against protected native primary inputs. It compares
the checked receipt, verifies the full retained object closure and write denial,
and exercises missing/corrupt bytes, changed/unbound references, insufficient
reader capacity, cooperative protection refusal, malformed paths and oversized
receipt refusal. It then renders the remaining units, publishes their output
closures and removes every render directory from lookup. Episode assembly through
`episode_conform::build_with_scene_files` must independently recheck all four cached
deliveries and preserve every ordered frame and PCM sample. Missing output-provider
coverage refuses before creating an episode directory.
The same fixture places one output only in history storage, verifies that its
protected read comes from that tier, and requires assembly to leave it absent
from hot storage. Limits are per bundle; the caller still budgets episode fanout.
Windows CI explicitly runs
this external-tool case:

```powershell
cargo test --locked --features tig-publication --test tig_inputs fake_episode_lifecycle::cached_scene_bundle_rechecks_without_render_directory_and_rejects_bad_bytes -- --ignored --exact
```

Producer checklist review: Editor requires exact master/plan output checks; Sound
Designer retains the independent decoded PCM checks; Rights and Provenance Steward
requires generated inputs only, no external upload and no promotion of technical
success to creative approval. These are applied checklist perspectives, not
independent human signoffs. Existing real project caches and bindings are untouched.

This implements cached scene-output reading and episode conform consumption.
Source/control JSON and presentation-template segments retain their explicit local
root contracts; the native-scene provider has no fallback to those roots. Semantic
scene scope, selected receipt/master pins and full native delivery checks remain
mandatory. This API does not turn compiled scene masters into primary dependencies.

Managed publication currently selects
explicit mapped outputs rather than automatically publishing every intermediate;
the fixture explicitly publishes its complete receipt closure. A production
output-bundle publication command, native cache-backed presentation consumption,
and producer/context-driven complete episode execution remain required before
claiming production project readiness.
