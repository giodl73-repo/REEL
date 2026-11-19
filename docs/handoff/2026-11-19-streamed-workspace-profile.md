# Versioned owned workspace capacity

`reel.workflow-staging.v2` permits an explicitly declared workspace reservation
up to 128 GiB. The original v1 profile retains its 512 MiB ceiling. Both require
a valid capacity policy whose `max_object_bytes` covers the whole requested
reservation; neither infers available space from an authored budget. Admission
still uses the existing selected-volume capacity coordinator and policy.

V2 owned intents use `reel.workflow-owned-intent.v2`, binding the exact normalized
profile hash together with the complete file manifest. Reopen checks the
original parent/directory identities and immutable intent. Changing the profile
budget, policy or version refuses against that owned context. V1 intent bytes
remain the original unwrapped manifest. Existing tickets keep their own pinned
profile and reservation; no profile upgrade or new reservation occurs on resume.
The shared ledger separately requires the exact original reservation ID, volume,
path and byte count, with exclusive live writer ownership. Timeout/process exit
does not release preserved capacity or authorize scratch deletion.

This profile is shared by the existing input staging, worker and movie-comparison
workspace mechanisms. It changes no task/native admission, file count, changed-
path, actor, comic/media contract or film execution limit. The existing bounded
part/link protocol, guarded files, completion checks and no-overwrite behavior
remain in place. It adds no cache engine or unbounded job scheduling.

Two new regular component tests check explicit large-profile admission rules,
unchanged v1 intent bytes and refusal of changed v2 budgets/policies/versions.
A separate opt-in fixture is prepared to reuse the already-qualified synthetic
3,600,000,001-byte file in a 32 GiB owned workspace reservation. It copies that
existing source into a producer-owned in-place file, interrupts after a small
control part is flushed, and invokes only the exact frozen recovery fixture in
a new process. Recovery must reopen the original capacity/ticket/owner, refuse
changed reservation/policy values, preserve the large file without a copy part,
complete materialization and hold mutation guards. Both source and destination
scratch/capacity remain preserved. This is a filesystem/capacity component test;
it creates no native job, publishes no asset and grants no film/task completion.
Actual source/tool/result pins are recorded below.

## Qualification result

The fixture passed in 115.07 seconds, including a 63.22-second child recovery
process. It reused the original 32 GiB reservation/ticket in preserved root
`.tmpKKxLxc`, refused changed reservation bytes and policy, verified the complete
3,600,000,001-byte producer-owned file and completed materialization without
creating a copy part for that file. Mutation was refused while file guards were
live. The original ticket stayed byte-identical. The source's earlier 4 GiB
reservation and this workspace's 32 GiB reservation/scratch remain preserved.
No native job, render, asset publication, retirement or project adoption occurred.

Source: `4b89f6bb72c4df163e1119bdb80e59b662b6e0d4`; all 1,936 tracked files
are archived and individually hash/length verified. Frozen release test tool:
SHA256 `efcfb3f64e87aaa3e152942f0e7fdb2d58e621de2801d572b38e48285fa0ea15`,
15,368,192 bytes, with film support. All 75 standard native tests, 247 SDK pins
and strict all-target Clippy with/without film pass.

Private evidence certificate:
`F:/src/tig-dev-temp/reel-streamed-workspace-20261119-v1/qualified-tools.json`,
SHA256 `e12550a48c00b03e135430e0a1d7dee9a99992b402d7b90dc3ad1f63f6ff7cc0`.
It binds the frozen source/tool, parent/child logs, terminal timing/outcome,
original recovery ticket/pin, immutable owned intent and completion marker.
Session 1537 terminated successfully; do not create a fresh qualification to
resume or inspect this original workspace. Its original recovery entry point
requires the preserved root and externally pinned original ticket.

The v2 native film workflow remains gated. Large snapshot/projection admission,
candidate/retained return budgets, composition, process execution/interruption
and actual owned project adoption/rollback still require implementation and
qualification. An original-profile recovery check is not an approved profile
adoption or a selection transaction. S4/S5 jobs, recordings and approvals stay
untouched.

Producer-applied editor and rights/provenance checklists preserve immutable
source lineage, exact review/approval subjects and the separation of technical
capacity from creative, translation, voice, likeness and release authority.
