# Frozen generic job CLI review

Target: `run-frozen`, its bounded private admission and managed control-edit
fixture. Parent PR 271 supplies the candidate worker; this change supplies a CLI
to the runner APIs already exercised there. It introduces no enrollment engine,
provider, schema interpretation or production runtime installation.

Producer-applied review uses REEL rights/provenance and editing lenses, plus the
existing Tig concurrency/recovery and Git/adoption checklists. These findings
are not independent review or human acceptance.

| Finding | Disposition | Owner / verification |
| --- | --- | --- |
| File presence must not grant source ownership | Replay external source/job admission; existing runner checks journal owner, selection and revision under its scoped fence | Tooling; wrong-owner and stale-revision fixtures |
| Stored policy cannot admit its own meaning | External profiles, consumption and operation replay the retained context and exact job tree | Tooling; changed checkpoint, consumption and selection refuse |
| An unpinned request could execute different work | Raw manifest matches FileRef and runtime `request` pin; Windows read handles exclude write/delete | Tooling; request/runtime corruption refuse; trusted stable namespace elsewhere |
| Candidate success could imply approved rendering | Null production claim and false creative approval; no native producer or selection rewrite | Tooling; output and reopened retention-header checks |
| Git could prematurely release byte obligations | Existing result owner remains after both outputs are retained and an isolated proposal is created | Consumer; journal/result readback before proposal |
| CLI status must match the generic runner contract | Failed children return errors; only a successful return exposes `succeeded: true` | Tooling; real CLI exit-status assertion |
| Errors could leak private paths or text | Fixed public failure; detailed successful runner JSON stays private | Tooling; refusal stderr checks |

Actual project enrollment/composition and consumer handoff remain open. Source
authority cannot be inferred from the candidate's declared checkpoint.
Non-Windows hostile namespace races, native editor semantics and production
cutover are outside this command's claims.

The first compile exposed a fixture serializing an intentionally deserialize-only
Config; it now supplies explicit private admission JSON. The first CLI run
exposed the generic runner's lack of a `succeeded` field on success. The wrapper
now sets it only after successful return. No runner, fence or retention check was
relaxed.
