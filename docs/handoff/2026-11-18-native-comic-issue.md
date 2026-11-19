# Native complete comic issue assembly

The explicit `comic-issue-assembly` workflow capability binds a full publication,
edition and issue plan to every required part receipt, editable source and named
output. The native adapter replays the complete assembly through declared input
readers. Owned worker dispatch, returned-file checks, retained proof and editable
export use this capability separately from `comic-render`.

Part requests can declare `part_id`. Their output identities become
`PUBLICATION/EDITION/parts/PART/OUTPUT`, preventing different parts' reading PDFs
and transcripts from colliding in a child's declared inputs. Request and receipt
fields are optional and omitted for older unscoped work. Historical receipts
keep their original identities; no migration or regeneration is implicit.
An explicitly scoped request must match its selected workflow part and the
assembler's part ID. Final issue derivatives use the edition's output identities.

Part outputs and worker receipts retain their exact upstream selectors. Editable
sources retain their separately declared Git or upstream origins. The assembler
does not derive native input authority from a source-object list or copy an
undeclared worker folder. Returned packets require the exact complete file set,
including editable parts and relocated controls. Retained replay rebuilds those
controls from admitted inputs, then checks native receipt and derivative bytes.

This fixture's deterministic assembly request is pinned in the input commit.
When a controller derives a request after unknown provider results arrive, it
must retain that control artifact as its own task output and bind it upstream,
or use an explicit registration transaction. It cannot inject a new record into
the frozen commit. Automatic derived-control production/scheduling still needs
its own implementation and qualification; this fixture does not establish it.

All existing limits remain: 128 inputs/64 MiB closure, 128 changed paths per
native phase, 256 owned staged files and explicit existing actors. Whole-issue
metadata can describe larger work than one native job can admit. Oversized work
still requires authored grouping and further scheduling; this adapter does not
allocate actors, retire scratch, release claims or select candidates.

The two-part synthetic fixture targets separate parent slots, retained part
results, child-owned composite admission, exact-runtime issue dispatch, retained
completion, source-actor coverage loss, bare-history reopening, editable export,
local issue reassembly, restoration and a fresh text edit preserving clean art.
Its original operation/control roots are preserved even on failure.

| Producer checklist | Finding and disposition |
| --- | --- |
| Comics/editorial | Stable part identities prevent reading-output collisions; master/page order and editable lettering remain checked. Larger issue rhythm and actual bilingual consumer qualification remain open. |
| Platform/audience | Reading PDF, source transcript and editable packets remain separate. These are private digital reading derivatives, with no print or accessibility certification. |
| Rights/provenance | Original source/part origins, pinned fonts and license evidence remain bound. No external service is called; technical receipts grant no creative, translation, font, likeness or release approval. |
| Concurrency/recovery | Parent retention is checked before child admission. Shared retained replay uses sealed custody permits; owned work and original identities persist on uncertainty. Synthetic coverage removal does not qualify production slot retirement. |

All 65 shared tests, all 37 default native tests and strict shared/native Clippy
passed on the initial build. Its native issue fixture retained both parents, then
refused child preparation with `admission-atomic-claim-bound`, before first child
acceptance or work staging. Original parent controls, claims and retained results
remain preserved. No uncertain effect is being retried under a fresh identity.

Issue jobs now use versioned projection v2 with flat ordinal storage paths and
an explicit logical-ID/path map. This preserves every declared payload, origin
and required metadata object while avoiding tree nodes introduced solely by
nested logical IDs. Other capabilities retain their v1 projection. The 128-path
guard remains unchanged. The revised build still needs actual qualification.

The revised build passes strict native Clippy, all 37 default native tests and
the CLI help check. Its reader also reopens both original v1 part results with
actual retained-owner verification. All 31 shared source/Cargo files match the
65-test shared source snapshot. It admitted the child with 119 declared objects,
then refused staging because that entrypoint still selected `comic-render`.
The admitted WorkRef, activation, original attempt, input ticket and capacity
reservation remain preserved.

The staging and guarded publication-input entrypoints now use explicit producer
dispatch. An additional recovery fixture reopens the original claim and ticket,
copies machine profiles pointing to the exact original frozen runtime, completes
input export with the corrected Rust adapter, and invokes that original runtime
for production and lifecycle effects. It creates no replacement admission or
attempt. A separate fresh fixture checks the corrected executable independently.
Both end-to-end checks are pending; all 37 default tests and strict Clippy pass
on this corrected build.

Initial failure and regression evidence is in
`F:/src/tig-dev-temp/reel-workflow-native-issue-20261118-v1`.
The admitted staging refusal and cross-version readback are in `...-v2`;
original-operation recovery and corrected fresh-run evidence are in `...-v3`.
