# Contained existing-renderer process port

`reel-workflow-native::film_process` (`film` feature, Windows) now runs the existing
scene renderer in the pinned native executable as a contained child. The parent
keeps its admitted `film_inputs::Inputs` alive, including exact native object,
namespace and installed program guards. Private child requests freeze original
control pins, physical input locators, FFmpeg/FFprobe, native runtime and limits.
They are execution intent, not transferable input authority or public receipts.

`render` requires an empty existing caller-owned workspace. It exclusively writes
and syncs `render-request.json` before launching the child. An existing workspace
or request refuses another render. `recover` requires the exact original request,
reopens every produced file against the delivery receipt and runs a distinct
check-only child; it does not synthesize another movie. Changed programs, input
bindings or limits cannot retarget recovery. `check-request.json` also remains a
single original launch; further interrupted-check reconciliation is still work.

Execution reuses `reel-assets::jobs::run_quiet`: atomic Windows Job Object
membership, kill-on-close, aggregate committed-memory/process limits and a finite
deadline. Stdout/stderr go to NUL. The child persists a bounded request-bound
structured outcome; absent or inconsistent outcomes require recovery. Process
failure/timeout preserves requests and scratch and does not retry effects.

Explicit limits cover frames, composition samples, decoded RGB bytes, final
output bytes and observed workspace bytes. Planning, probing, rendering and full
media verification all run inside the contained child. Workspace monitoring is
bounded and repeated during execution; it is not an OS disk quota and can observe
growth after it occurs. The existing 128 MiB result and 512 MiB scratch bounds are
not raised. Final inventory/pins are checked and read-only output guards survive
through `Rendered`; no ambient media-tool fallback exists.

The root renderer adds `render_with_control_files_preserving`, which retains its
failed staging directory. Existing entry points preserve their previous behavior.
The process port uses the preserving variant. Deadline termination likewise
leaves the original child staging bytes for diagnosis.

This port does not accept jobs, reserve capacity, register candidates, acknowledge
retained ownership, complete tasks, release claims, select media or retire scratch.
Its caller must establish workspace/capacity admission and connect the original
job lifecycle. Generic movie capability/dispatch, historical checking without the
original worker directory and retained movie completion remain open. Declared
runtime paths and hashes do not qualify neighboring DLLs or the entire environment.

Producer-applied editor and rights/provenance review: frame/sample limits must
refuse rather than shorten an edition; review IDs remain the root renderer's
compiled labels; child locators remain private. Technical checks grant no creative,
translation, voice/font, selection or release approval. No independent review is
claimed. Component and synthetic existing-admission qualification evidence will
be recorded separately after execution.
