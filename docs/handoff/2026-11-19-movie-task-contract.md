# Task-bound movie candidate production

`reel-workflow::film_render` binds a `FilmRender` tool to exact job/request and
production metadata, complete input aliases, FFmpeg/FFprobe artifact identities,
portable output paths and finite execution limits. The limits now share one Rust
definition with `film_process`; its existing request serialization is preserved.
Frame/sample limits are execution caps, not desired cel duration or drawing cadence.

The request explicitly maps every logical output to its packet-relative path.
`execution-request`, `execution-result` and `delivery-receipt` are mandatory JSON
outputs at `render-request.json`, `render-result.json` and `delivery/receipt.json`.
Other media remain beneath `delivery/`, preserving renderer-relative frame/font
paths. Missing/extra outputs, unsafe/reserved paths, case or directory aliases,
wrong media types, incomplete inputs and missing media programs refuse.

`reel-workflow-native::movie::generate` is an explicit production port over a live
admitted native input provider and caller-owned, capacity-admitted workspace. It
uses the pinned contained renderer and compares the declared root output set with
the compiled renderer contract before rendering. It returns candidate artifacts,
their physical mapping and a separate `receipt.json` bound to the task, original
source/request, auxiliary artifacts and exact output identities. The root delivery
receipt stays intact under `delivery/receipt.json`.

Native input/output/program guards remain held through the returned production
object. The result, including the separate worker receipt, must fit the existing
128 MiB ingress bound. No payload is published, job completed, selection applied,
capacity released or scratch retired by this port. Execution requests contain
private machine locators and must not be treated as public film deliveries.

Automatic `FilmRender` worker/dispatch remains Unsupported. Independent returned
media verification, native retained-history checks and original interrupted-job
reconciliation must be connected before enabling that route. A candidate receipt
is not authenticated execution or retained ownership. The pure binding and file
correspondence helper also grant no media validity or custody by themselves.

Five component tests cover nested paths and receipt separation, task invalidation
from changed render limits, unsafe paths/evidence/type refusal, exact metadata and
input/program closure, and cross-task bound reuse. A synthetic native fixture
prepares an original job and owned capacity through existing Rust interfaces,
then checks all twelve candidate outputs/thirteen packet files and unchanged
claims. Actual fixture evidence will be recorded after execution.

Producer-applied editor and rights/provenance review: limits must refuse instead
of shortening the film; authored timing and selected cue recordings remain exact
inputs. Candidate receipts explicitly leave human approval and retained-owner
acknowledgement false. No creative, translation, likeness/voice, font, selection
or release authority is inferred. This is a checklist application, not independent
review or an authority decision.
