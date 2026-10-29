# Direct native episode controls without a checkout

The direct-native episode path now resolves parent controls and each scene's job,
cue contract, production metadata and semantic selection through protected named
providers. Scene primary media, delivery outputs and media programs retain their
separate providers. No scene-control provider refusal falls back to a local source
tree. Existing local-root APIs continue through the same planning/checking logic.

The cue compiler accepts verified production bytes through an additive pure entry
point. Scene planning factors its existing validation/media logic behind either
the original local loader or an exact provider loader. Semantic planning similarly
shares its existing selection/event checks. Virtual relative names resolve against
the original control name, never the physical CAS object's directory. Production
metadata must match the declared job digest and cue-contract scope. Metadata reads
are bounded and independently checked against their declared length/digest.

ControlTree uses the existing canonical Tig tree and metadata-provider format for
caller-admitted derived control names. No new authored inventory or source grammar
is introduced. It reads at most 64 names and 1 MiB of tree metadata, holds at most
16 MiB of unique control bodies with 4 MiB per body, enforces configured cache
reader/request/object/byte limits and exposes the exact held set. It neither
discovers caches nor copies media. Opening it grants byte/path protection, not
source currency, custody, approval or deletion permission.

The episode provider map must exactly cover direct native scenes. Missing/extra
coverage, wrong production scope and explicit semantic-provider refusal fail before
output publication. Imported and presentation-specific routes deliberately refuse
this entry point until their own complete control adapters are supported.

The generated cached episode fixture now persists four controls per scene in a
Tig metadata tree, reopens that tree and obtains the bodies from existing synthetic
caches. It hides its entire temporary source repository and passes nonexistent
source/asset roots during real native assembly. All source/primary/output/runtime
owners remain held; full native media and ordered frame/PCM/timestamp checks run.
The temporary repository is restored before checking the result. No actual project
checkout is renamed or read by these fixture operations.

Producer Editor/Sound/Rights review: preserve original named controls, graph/event
semantics, source scope, exact selected output bytes and full media validation.
Do not use metadata-provider presence as authority to select or publish a work.
Review lenses are producer checklists, not independent or human approval.

Executed local qualification: final all-target/all-feature locked Clippy passes
(128s); the final complete source-independent cached fixture passes (202.59s),
including absent roots/checkout and refusal cases. The prior version before the
final scope/budget assertions passed (194.84s). The complete library suite,
standalone thirteen-test episode suite, v1/edit/v2/Git/cache fixture and normal
native-consumer suite all pass; the latter reports 71 passes and 12 explicit external
cases ignored (269.93s). Both full media fixtures were explicitly run. Optional
Tig-only test compilation passes (30.64s). An initial refactor's needless borrows
were corrected without altering validators. Final diff/format checks pass.

This closes direct-native source-control filesystem dependencies. It does not yet
create checked parent production claims, complete publication/retention/child
retirement, implement generated-primary freshness or presentation eligibility,
classify compatibility packages, adopt an installed runtime or perform a project
cutover. All such source and lifecycle assertions remain separate requirements.
Exact-head hosted qualification remains required before merging.
