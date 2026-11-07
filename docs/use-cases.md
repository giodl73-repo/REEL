# Use cases and workflow choices

REEL accepts project-owned declarative controls and exact media identities. Choose
a workflow by the artifact you want to produce. [Getting started](getting-started.md)
provides a runnable planning example; the linked contracts below specify the
project inputs needed for production.

## Assemble an illustrated episode

**Inputs:** ordered scenes, measured cue recordings, selected cels, language
bindings, opening/credit presentations, an episode contract and runtime budgets.

Author semantic events using the [native scene contract](scene-authoring-v1.md).
Compile each scene's effective inputs for a chosen language. With Tig, capture
one committed view and [resolve current leaf bindings](tig-native-scene.md), then
run an admitted [managed scene job](tig-job-runner.md). Independently check the
rendered frames and samples before retaining a component proof.

The [episode planner](tig-episode-planning.md) resolves declared component order.
The [episode job](tig-episode-jobs.md) obtains matching current component proofs,
verifies the parent/child controls and assembles the output. `verify-episode`
checks components without producing the episode movie; `run-episode` assembles
it. `finish-episode` can recover a completed job's metadata/ownership handoff
without rendering again.

**Result:** a checked episode candidate with retained evidence and explicit
record proposals. A project still chooses the release version and delivery.

## Make a music video or a sequence of videos

**Inputs:** an exact recording, picture events, frame/sample timing, caption
choices and a delivery profile. A music video may use a recording clock instead
of speech-alignment cues; see [sample-clock controls](tig-sample-clock.md) and
[delivery templates](tig-delivery-template.md).

Build the sequence from declared pictures and recording bytes. For reused
finished segments, use their exact selected media and matching
[retained presentation contract](tig-retained-presentation.md). Declare the
order, joins and output requirements in the composition. Different dimensions,
frame rates, codecs or audio policies may require an explicit conform step;
similar filenames do not establish compatible segments.

**Result:** a new video/composition candidate whose source selections and reuse
are inspectable. REEL does not infer a playlist, automatically adopt every
upstream revision or promise that every reuse avoids transcoding.

## Fix one cel while keeping the rest

**Example:** a reviewer requests a new ending picture; the recording and earlier
pictures should stay exactly the same.

Create new picture bytes, update the project record/selection and commit the
change. In the [Tig scene compiler](tig-native-scene.md), the parent binding map
can keep the same logical ID while resolving its new selected bytes. The old
job's expected pin must refuse the changed input. Recompile against the new
view and render/check a new candidate.

The [two-version fixture](tig-public-integration.md#try-the-two-version-example)
proves this with two cels and one shared cue: only the replaced cel's decoded
frames change, and shared PCM stays identical. Declared
[changed-only planning](changed-only-build-v0.2.41.md) and native recipe comparison
can identify reuse opportunities; complete dependency declarations remain the
producer's responsibility.

**Result:** a reviewable change with the previous version still identifiable.
Storage demotion is a separate policy decision after other references and
verified copies have been accounted for.

## Share pictures across languages

**Inputs:** a common semantic cue/event spine, language-local text/recordings,
measured alignment and explicit shared/override bindings.

[Scene authoring V2](scene-authoring-v1.md) records shared event intent once.
Each language supplies its own realization and timing. The
[Tig binding map](tig-native-scene.md) can share pictures while overriding only
declared language-specific assets. A missing selected variant refuses instead
of silently using another language. The consumed recipe determines which edits
invalidate each language's render.

**Result:** independently checked language outputs with deliberate shared
inputs. Translation approval, spoken-text accuracy and musical adaptation
remain their own project decisions.

## Edit cels, notes or lyric underlay visually

Choose the matching native tool:

| Edit | Source and editor | Apply result |
| --- | --- | --- |
| Picture order, holds, comments and supported insertion/substitution | [Cel project mapping](cel-sorter-project-v1.md), [cel sorter](cel-sorter-v1.md), [insertion](cel-insertion-v1.md) | New native draft controls with exact source locks |
| Existing note pitch and tick timing | [Note workshop](music-note-editor-v1.md) | Unreviewed native music-model proposal |
| Exact-text assignment to existing notes | [Lyric underlay workshop](music-underlay-editor-v1.md) | Source-locked underlay proposal |

Export the session, explicitly open it through the
[local editor host](editor-host-v1.md), save a draft and apply it with the native
tool. Then compile/check and review the candidate. Browser preview timing or a
successful download does not prove rendered media correctness. The
[WASM graph workbench](https://giodl73-repo.github.io/REEL/assembly/) is a separate
dependency inspector, not one of these media editors.

## Produce songs, scores and arrangements

Keep original text, recording, analysis, editable model and arrangement as
separate governed inputs. [Music intake](music-interchange-intake-v0.3.2.md),
[model drafting](music-model-draft-v0.3.5.md),
[score export](music-score-export-v0.3.1.md) and
[arrangement plans](music-arrangement-plan-v0.3.10.md) define their own contracts.

For repair, [native song editions](NATIVE-SONG-EDITION.md) and the
[masked repair core](original-song-masked-repair.md) preserve protected samples
and expose explicit candidate boundaries. Recognition/provider workflows and
older engraving adapters have additional runtime and review requirements;
see [instrumental scoring](music-instrumental-score.md) and
[legacy collection exports](SONGBOOK-COLLECTION-EXPORTS.md). Rust orchestration
does not mean every optional provider or older adapter is implemented in Rust.

**Result:** explicit score, arrangement or repair candidates with provenance.
Machine checks do not listen, establish the intended melody, grant voice
consent or select the musical master.

## Master an album and create a companion video

**Inputs:** ordered, pinned complete recordings; integer gap lengths; explicit
FFmpeg identity; loudness/headroom targets and byte/time budgets.

[Native album mastering](NATIVE-ALBUM-MASTER.md) produces lossless/listening
masters, chapters and track metadata. Its checker recomputes complete joins and
sample transformations. [Album video](NATIVE-ALBUM-VIDEO.md) binds that packet to
explicit visual material and verifies audio/chapter correspondence. These
file-level producers can sit inside admitted project jobs; they do not discover
or select recordings for the project.

**Result:** a private engineering packet for listening and release review.

## Publish a songbook, manuscript or bilingual print collection

**Inputs:** ordered pinned text, paired language text, supported native score
SVG or retained PDF pages, explicit font files, page and output budgets.

[Native print collections](native-print-collections.md) provide `plan`, `render`
and `check` with the optional Rust `print` feature. The planner computes the
packet before output creation. Text is laid out from the captured source;
retained PDF pages preserve their declared order and content; supported native
score systems retain their notation geometry.

**Result:** a PDF collection, page previews and a private receipt. This renderer
does not translate text, recover editable notation from arbitrary scans or
implement every older Python collection/engraving feature.

## Track a delivery over time

**Inputs:** the exact output revision, a declared provider/target, project
delivery policy and retained evidence of what was published or observed.

Keep each target/language/profile explicit. A shared review folder and a public
video channel can have different policies and delivery histories. Use the
[Tig history host](tig-history.md) for its declared record/history operations,
and [delivery observation](tig-delivery-observation.md) to reconcile a saved,
pinned inventory with committed targets. A
[checksum-index association](tig-checksum-index-association.md) retains its
original evidence and reports its weaker scope; it is not provider readback.

**Result:** a reviewable record of the exact revision and evidence for each
target. These metadata adapters do not upload to Google Drive/YouTube or infer
what a remote target currently contains. Perform delivery through the project's
explicit publisher and retain the matching evidence before recording success.

## Manage capacity and old versions

Use a project's actual asset view and declared storage policy. Tig's generic
catalogue/storage services expose asset and byte placement outside REEL's media
semantics. REEL's existing
[accounting workflow](asset-manager-workflow-v1.md),
[version comparison](asset-version-diff-v1.md) and
[tier review](asset-tier-review-v1.md) support their documented registry/profile
contracts. Select the matching adapter instead of assuming those registry files
and Tig records are the same schema.

Plan and review first. Account for active worktrees/jobs and other consumers,
verify required surviving copies, then use the explicit permitted application
step. A deselected item may still be needed by another branch, frozen job or
historical retention rule. A current render or an old inventory alone grants
no deletion authority. Keep large inventories, raw reports and private receipts
in the project asset system.
