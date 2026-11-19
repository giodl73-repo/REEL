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
Actual source/tool/result pins follow execution.

The v2 native film workflow remains gated. Large snapshot/projection admission,
candidate/retained return budgets, composition, process execution/interruption
and actual owned project adoption/rollback still require implementation and
qualification. An original-profile recovery check is not an approved profile
adoption or a selection transaction. S4/S5 jobs, recordings and approvals stay
untouched.

Producer-applied editor and rights/provenance checklists preserve immutable
source lineage, exact review/approval subjects and the separation of technical
capacity from creative, translation, voice, likeness and release authority.
