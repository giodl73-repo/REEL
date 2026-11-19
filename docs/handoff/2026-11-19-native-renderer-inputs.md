# Native inputs for the existing film renderer

Optional feature `film` in `reel-workflow-native` compiles REEL's existing renderer
without enabling its older Tig feature. `cargo tree -i tig-core` confirms the
active SDK remains `vendor/tig-workflow`; no vendored SDK source is changed.
The separate native lock now pins the renderer's additional dependencies. This
does not upgrade the root producer's lock or establish a released distribution.

`film_inputs::Inputs::prepare` consumes an exact `NativeInputs`, an explicit
alias-to-logical-ID map and exact installed FFmpeg/FFprobe `Tool` pins. It refuses
an unadmitted reader: live original claims or a sealed retained-input permit are
required. Aliases cover every declared task input; unknown aliases and changed
hash/length references refuse without a filesystem or latest fallback.

For each distinct declared object, the bridge retains the enrolled native reader
gate before asking the same enrolled Annex runner for that exact key's content
location. It parses a bounded provider-issued suffix and opens it through the
existing qualified `tig_store::existing::ReadCache` with `PreventMutation`.
This second verified guard supplies REEL's subprocess path while retaining
namespace/file protection. It does not search for matching bytes or infer an
authored binding from a content hash. The original exact Git-record/annex-leaf
resolution and input authority stay alive in the bridge for the whole operation.

Installed program files are hash/length verified, and both file and ancestor
directory guards stay held. The `InputFiles` implementation overrides program
selection explicitly; a failure cannot fall back to PATH. Program declarations
have a canonical fingerprint that callers must freeze in their original
invocation/request. These live executable guards do not themselves authenticate
execution or establish durable retained ownership of installed toolchains.

Bounds: 128 declared inputs, 256 explicit aliases, 512-byte alias names,
512 MiB aggregate declared bytes, 128 distinct guarded provider files and a
4 KiB location report. This retains the existing single declared native-reader
source scope; it does not silently adopt the newer multi-pool reader or initialize
a cache/store. Existing typed provider errors retain missing, integrity, denied,
busy, unsupported and budget distinctions; unclassified observation uncertainty
requires diagnosis under the original operation.

This bridge deliberately grants **input access only**. A caller still needs owned
workspace/capacity admission, a pinned production invocation, response/candidate
registration, retained-owner acknowledgement and original-operation recovery.
It is not a new worker dispatch path, a movie retention proof or task completion.

The opt-in fixture imports exact files from the previously qualified synthetic
review-label scene into the explicitly declared existing native pool. It preserves
the packet, source, admission profile, original receipt, aliases and program pins
before renderer access. It then runs the existing renderer/checker directly on
native guarded paths, without exporting source copies for the renderer. It checks
48 frames, the frame-24 cel transition, source mutation refusal, unknown/stale
reference refusal and unchanged claims. Its rendered output remains local and
unselected; no movie is admitted as a retained task result by this fixture.
The fixture passes in 59.13 seconds: nine distinct native objects, 48 actual
frames, the frame-24 cel transition, refusal of a write-open against a live native
source, unknown/stale reference refusal and unchanged claims. The generated
contact image was inspected: `CEL red` and `CEL blue` remain legible. All 49
non-opt-in tests with feature `film`, strict Clippy and the original 247-file SDK
and managed-slot source-pin checks pass. The first negative fixture run failed
because its fake Annex descriptor had no installed bytes; it now uses a byte-valid
descriptor and proves refusal before any Annex/program/payload access. No real
project media or voices were used.

Evidence:
`F:/src/tig-dev-temp/reel-native-render-inputs-20261119-v1/qualified-tools.json`.
Source `25ef0af7f4ec5d2f51773bd72937ea98dcca40cd`; all 1,853 archived files
match. Runtime SHA256:
`c51fa45bf0ae69b2edf26a347723040c2c1bb30c27476aacccfbbd31de7bc684`.
Fixture SHA256:
`df1c953b11a02b83e7a72e8024eeb43afb182ac016df8151a510df9d1392855b`.
Preserve controller `F:/src/tig-dev-temp/.tmpkr4voL`, source
`F:/src/tig-dev-temp/.tmpI2P6r5`, original admission/claims and the local
`review-film` output. The original job remains protected; this input/renderer
qualification neither retains the movie nor closes/reuses its slot.

Producer-applied custody/recovery checks preserve original claims and receipts;
editing/sound checks retain compiled frame/sample clocks and the existing media
checks; rights/provenance checks keep source/font identity, explicit tool binding
and private evidence. These are checklists, not independent review, consent,
creative approval or public-release authorization.

Next integration: bind auxiliary media-tool objects into the film task/invocation,
connect this provider to owned deterministic movie production and existing native
result/retention, then qualify interruption recovery and historical movie access.
The existing renderer and checker remain the production implementation.
