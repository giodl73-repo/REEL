# Private-review admission producer review

Entry `32f21dd45dbf59726c7286494a080e826ee2c2aa`; scope, controls and
L0/L1/L2 checks are in [the contract](../delivery-private-review.md).

Rights/provenance lens: explicit private permission belongs to the trusted
caller, with exact checkpoint and instance pins, not a status invented by the
renderer. Source statuses and existing approvals remain unchanged. Rejected,
withdrawn, blocked, malformed and unknown selections refuse even with a matching
pin. This API grants no rights, consent, release, independent custody or eviction.
Other rights restrictions still need the caller's independent admission.

Platform/audience lens: review purpose is private and an unapproved candidate
remains a candidate after rendering. The existing worker has no automatic
external delivery. Exact admission checks and retained managed-job input evidence
prevent silently replaying a draft as production-selected. A later selection or publication decision must
bind the actual result separately; a technically matching recipe cannot make it.

Identity/storage lens: the exception list is generated operational admission,
not an authored asset catalog. It cannot include unrelated/unused records,
advance with a branch, or replace admitted type/payload requirements. Source
guards, metadata archives, runner capacity, retention and Git proposals remain
the existing mechanisms. Generic fixtures use synthetic assets only.

The first projection test found Tig's deliberate descriptor/operation-parameter
equality fence: changing only the recipe parameter object refused managed freeze.
The implementation preserves that invariant and uses the existing input-evidence
dependency role for permission instead. Annotation-only permission refresh and
separate selection preserve consumed fingerprints while old grants still refuse.
The next native run exposed the ordinary runner's intentional evidence-role
refusal. A separate retention-only route preserves that entry point and keeps
Evidence excluded from Input authority; native tests explicitly attempt that
role substitution. Composition/claimed outputs still refuse on the new route.

Final local validation: 20 regular delivery-template cases pass (four native
cases explicitly ignored in that command). Both production-selected and private
review managed native cases pass in 540.42 seconds, including archived source,
SDK/capture/contained CLI, source ownership, publication/retention and Git handoff.
Missing retained permission evidence refuses readback; the private case rejects
using that evidence as a media input and verifies the old runner still refuses
its context. Locked all-target/all-feature Clippy passes with warnings denied.
Formatting and whitespace checks pass. Earlier failed/intermediate logs remain
separate; hosted qualification and actual project adoption are still open.
This is producer review, not independent or human approval.
