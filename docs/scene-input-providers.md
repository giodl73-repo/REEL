# Scene delivery input providers

Design and implementation scope: native scene delivery can consume exact input
files supplied by an application provider, including a captured Tig byte view.
Initial baseline: REEL ab14406bd1d5786f90bbb9f9d99af82dcd29b2c4; rebased onto
4465d43f8ed808e7c4f9dc5f613b9a23627a3971 after the alignment reader merged.
This work does not change the job schema, output receipt schema, creative selection or cache policy.

`scene_delivery::InputFiles` resolves a job `FileRef` to a verified local path.
The existing `Path` and `PathBuf` implementations retain root-relative exact-case
file checks. An injected provider interprets `FileRef.path` as an application-owned
logical name and must match its SHA-256 and size exactly. A logical name is not
an instruction to search disks or choose a newer version.

The Rust entry points are `plan_with_inputs`, `render_with_inputs`,
`render_audio_with_inputs`, `check_with_inputs` and `check_audio_with_inputs`.
They borrow `&(impl InputFiles + ?Sized)`. The original `plan`, `render`,
`render_audio`, `check` and `check_audio` signatures keep accepting a local root
and delegate to the same implementation. Existing command-line flows remain
root-relative; applications explicitly opt into an injected provider.

An injected provider must hold effective protection over the returned exact bytes
and their path for its entire borrowed lifetime, through FFprobe, FFmpeg and final
checks. It must refuse when it cannot provide that guarantee. The renderer borrows
the provider for the synchronous operation. Implementations are trusted adapters;
a trait cannot sandbox a provider that lies about integrity or drops its guards
through interior mutability. This obligation is stronger than the historical
root-relative implementation, which retains its original checks and behavior.

For Tig, the application captures a source checkpoint, resolves explicit IDs and
stream selectors under exact admitted definitions, compares every expected pin,
and holds the resulting byte readers. Windows protected readers can supply paths;
cooperative read/seek readers cannot. Keep guards in the provider through return
from rendering. Do not release a guard after obtaining a path or substitute a
disposable LFS download alias. Other platforms need a provider with equivalent
path protection; they must not silently downgrade to cooperative handle mode.

```text
Git snapshot -> Tig logical ID -> expected object pin -> protected byte reader
                                                              |
job FileRef: name + hash + size -> InputFiles -> held local path
                                            |
                            plan / probe / render / independent check
                                            |
                                 render returns; guards may drop
```

Only media and asset-side evidence use this provider. Job files, assembly
contracts and their production manifests remain explicit local authored inputs;
outputs are independently checked in their staging/output directory. No cache
initialization, new media staging layer, automatic transfer, daemon or cache
deletion is added. Existing renderer-created output/overlay staging is unchanged.
Receipt identities keep logical names and exact pins, not provider cache paths.

Verification must include legacy/local plan parity, an explicit mapped provider
whose input names have no corresponding worktree files, missing/stale pin refusal
before output publication, real picture/audio render and recheck, and a Tig-backed
protected-reader rehearsal. Full native CAMINOS family parity, job/source lifetime
claims, custody, eviction and runtime adoption remain separate requirements.

Local Windows evidence (2026-10-26): the mapped-provider test preserves the local
plan's exact clocks and refuses missing/stale pins before output creation. The
explicit FFmpeg case compares all decoded picture/master/review frames and
D/M/E/mix/master PCM with local-root rendering; full and audio-only independent
checks pass. Current-head cross-platform CI must still execute it.

The full scene-delivery target passed 25 tests with 14 external-tool cases
ignored; the new real-provider case was also run explicitly and passed. The
all-feature root library suite initially passed 179 with eight ignored cases.
After rebasing onto the merged alignment-reader change, it passed 183 with eight
ignored cases. All-target/all-feature warnings-denied Clippy, formatting and
whitespace checks passed again on that rebased source.

An external temporary Rust rehearsal used actual Tig
`d754d2e587f2da0041901ed30de3f7cc0c7c42b8`, without adding a private Git dependency
to REEL. A captured Git view selected two cels and one shared cue under an exact
admitted definition. A provider held three `VerifiedRead` guards in mandatory
Windows protection mode while REEL rendered and rechecked the scene. Attempts to
write/delete a held source refused; creating a new unrelated object succeeded.
A second Git revision changed only cel B. Its new render preserved first-half
pixels and all mixed PCM while changing second-half pixels; stale expected pins
refused. The old captured view rechecked the first delivery after restoration of
the exact original job bytes. All guards released; four small synthetic source
objects remained before temporary-fixture cleanup. This is local synthetic
integration evidence, not private episode parity or cross-repository CI.

The first historical rehearsal serialized a parsed job rather than restoring its
original bytes. The existing receipt check correctly refused that changed job
hash. Replaying the exact original buffer passed. Restore byte-pinned job/contract
inputs exactly; equivalent-looking JSON is a different file revision.

Normal synchronous subprocess completion is exercised. Abnormal child/process
recovery, complete job claims and source lifetime after an operating-system IO
failure remain open; this provider interface does not establish eviction authority.
