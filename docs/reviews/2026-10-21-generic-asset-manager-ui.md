# Generic asset-manager review

Producer-applied checklists: Platform and Audience; Rights and Provenance Steward.
These are tool/design reviews, not independent agent runs or human approvals.

Platform findings: the existing offline review now supports observed-tier
filtering, size/reclaim sorting, chosen-only inspection and explicit hold reasons.
Each block reveals up to eight hashes and explains that its version reference is
not exclusive ownership. Logical content, observed copies and estimated physical
reclaim remain distinct. Unknown allocation stays unknown. The chosen set starts
empty; pagination and batch/expiry gates constrain decision export. Six shipped
script interaction tests cover those behaviors. Browser layout inspection was
unavailable in this session; no visual browser acceptance is claimed.

Provenance findings: project IDs and paths are configured in the producer repo.
The new path-policy opt-in removes host-specific drive-letter assumptions while
preserving old consumers' defaults, local-root validation and independent-volume
requirements. The new census is explicit; viewing a saved review never refreshes
it or accesses an archive. Raw census identity is frozen into a private policy
copy. Original controls remain unchanged and are still checked before requests
and application. Reports, preview hashes and consumer paths stay private assets.
No production payload, inventory or receipt is included in this public change.

Technical validation: 114 Windows Rust tests (111 unit, three CLI/concurrency
integration), strict crate clippy and six JavaScript interaction tests passed.
A separate synthetic project on two real local volumes exercised the actual CLI
refresh-to-HTML-to-exact-request-to-hash-verified-dry-run flow. Its current input
was held, its explicitly used prior version was eligible and its unregistered
content remained archive-first. No fixture payload was removed by that flow and
the configured offline archive was not created. Unit tests additionally verify
new/unregistered additions and removals are reflected by explicit census and
same-volume retention fails closed. Existing corruption, interruption recovery,
consumer protection and concurrent-producer fixtures remain passing.

Scope limits: private consumer runtime adoption is separate. Historical removal
still has its legacy job-admission gate; concurrent creation remains supported.
Copy-first movement, packed archive restoration, cloud storage and permanent
last-copy retirement are not implemented by this change. Browser reviews remain
offline decision exports; execution and journal inspection use the native CLI.
