# Native cel source inspection

`reel-assets --root <existing-directory> cel-inspect <scene.json> <scoped-bindings.json> <new-private-report.json>`

reads only the two explicitly named controls. The library API is
`reel_assets::cel_source::{read, inspect}`. It needs no movie, cache, source checkout,
catalog, fixed project folder layout, server or generated alignment. Each control
is bounded to 1 MiB; there are at most 16 languages, 4096 cues per language and
8192 total cel events. Reports use `reel.cel-source-report.v1`; successful stdout
uses `reel.cel-source-summary.v1` with counts and booleans only.

V1 language-local events and V2 shared events use REEL's existing native types.
V2 discovery calls native materialization, including scoped language picture
overrides and canonical cue coverage. A row retains language, local cue/event,
shared semantic identity when present, trigger, graph slot and exact picture
binding. Repeated selected pictures/slots retain distinct event identities.
The original scene/bindings bytes have separate SHA-256 locks in the private
report. No input is rewritten or normalized for these locks.

Unsupported native fields/schemas, mismatched binding scope, ambiguous event
identity, unbound pictures, invalid asset identities and incomplete ready lanes
reject. Accepted imported evidence remains explicitly marked; it does not become
render-ready authoring. Some historical migration wrappers have fields outside
the native types and reject rather than silently losing those fields.

The report is private derived data: paths, IDs and asset identities must not be
copied into public fixtures. Retain production reports in the project's existing
cache/LFS if they need durable custody. The command creates a new report file and
refuses an existing destination. It neither registers that file nor writes media.

This is the shared discovery boundary for authoring tools, **not a completed
cel-sorter editor**. It does not measure alignment, infer holds, validate a full
native build, certify selected assets/custody, or grant human approval. The
current music-video editor's export/apply UI is a separate producer-owned port;
multi-cue discovery here does not imply that editor supports multi-cue edits.
Full source/graph validation, optimistic edit application and measured alignment
remain required before a new draft can compile or become a selected version.

Synthetic tests cover bilingual/multi-cue V1/V2 discovery, shared overrides,
stable IDs/reused slots, exact-byte locks, invalid scope/schema/fields/assets,
duplicate/mis-scoped events, incomplete lanes, imported evidence, control bounds,
private stdout and no-overwrite executable behavior. No cache or media is read.
