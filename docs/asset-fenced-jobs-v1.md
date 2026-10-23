# Opt-in native job publication enrollment

`reel-assets --root <project> run-job-fenced <manifest.json> <existing-fence-directory>`
executes an explicitly requested trusted native job with durable input/output
demand and short per-asset publication fences. This is an execution command,
not a preflight or sandbox. Startup does not execute, resume, render or hydrate.

The manifest's native `retain_workspace` choice is honored. Retained jobs keep
bounded scratch for explicit verified discard. Automatic jobs publish declared
outputs, request and diagnostic evidence, then register/verify those artifacts
before retiring only their owned scratch. There is no new expiry, daemon or
job replay. Existing unenrolled jobs and installed bindings are not switched.

An explicitly installed [producer-family cache requirement](asset-publication-fence-v1.md#explicit-producer-family-requirement)
changes intake for updated binaries: ordinary jobs refuse required caches before
admission, and fenced jobs must match every configured active-cache requirement.
Accepted controls stay pinned through the job without lifetime asset range locks.
Multiple required domains currently hold intake pending cross-cache demand routing.
Installation requires quiescent producers and does not certify other writer families.

The domain must already exist in a separate metadata child of the cache chosen
by capacity admission. A different admitted cache is not silently given another
domain. No cache, control or archive fallback is initialized. Canonical inputs/
CAS tools must be registered; installed runtimes remain separately pinned
external installations, not inferred CAS demand.

Intake reads at most 256 KiB and parses the exact bytes it records/hashes, with
UTF-8 BOM handling. This also removes ordinary intake's second-read race.
Enrollment permits 512 distinct input/tool hashes and bounded output roles.
Root `job-publication.json`, `job-output-intent.json` and
`job-retirement-intent.json` paths are reserved. Retained jobs use enrollment
version 1; automatic jobs require version 2. Schema/retention mismatches reject.

## Ordering and bounds

1. Validate exact registration/budget and reserve capacity.
2. Acquire shared input/tool hash fences; sync compact enrollment control and
   exact `job.json`. Enrollment binds job ID, manifest hash, cache/workspace
   and physical fence identity. A partial pair fails collection.
3. Release fences before input resolution, verification, linking/copying or
   execution. Durable demand protects ongoing work; lock release is no permit.
4. Verify local outputs. Acquire shared output fences only to sync output
   intent, then release before canonical CAS verification/copy/publication.
5. Sync an enrolled result without overwriting an existing control. Its output
   roles/hashes/sizes must match the output intent.
6. Automatic retirement prepares request/evidence files and describes bounded
   salvage outputs before publishing them. Shared artifact fences cover only
   syncing one compact retirement intent, then release before CAS verification/
   copy. Register all retained artifacts and verify custody before scratch unlink.

Input-fence refusal precedes workspace creation and input payload access. Later
failures attempt the requested bounded salvage/retirement lifecycle; retention
failures retain demand for explicit reconciliation. Salvage cannot bypass an
output fence. Explicit reinvocation creates a new job, never hidden execution
replay. A failed enrolled retirement is not automatically retried or rewritten.
No scratch crawl infers undeclared outputs. Fences do not span execution, media
hashing or copying.

The native payload/scratch budget additionally covers manifest bytes and three
64 KiB control allowances. Enrollment, output intent and result each have that
individual limit. This is logical allowance, not exact physical allocation.
Existing native execution/output-count/timeout limits remain.
Automatic jobs additionally budget two request-size copies, two encoded evidence
copies, a 256 KiB registration record and a 64 KiB retirement intent. Encoded
evidence is bounded to 2 MiB plus 64 KiB; decoded diagnostic payloads retain the
native 2 MiB bound. Evidence bodies remain CAS assets, not Git records. The
retirement intent contains at most 64 output descriptors and two control descriptors.

## Collection, discard and privacy

The native-job collector verifies control scope/manifest/domain, meters/stamps
controls and keeps input protection separate from output demand. Output intent
holds before CAS publication/result; it is not custody or a build-input edge.
Missing/recreated/corrupt controls and enrolled results without matching intent
fail collection rather than producing zero references.
Retirement outputs remain output demand; request/evidence are control-artifact
demand. Neither intent is custody proof. Missing/corrupt retirement controls and
unsupported bindings reject rather than returning a partial successful scope.

Exact registered-manifest discard validates enrollment/domain before removing
an enrolled workspace. Existing admission, registration, retained-original/output
verification and explicit incomplete-discard checks remain. No canonical cache
deletion is added. Automatic retirement uses the native private-candidate
registration workflow, without creative selection or approval. If retirement
intent exists, explicit discard also requires every recorded artifact to be
registered and freshly verified. Enrolled cleanup pins immutable canonical files
and ancestors through scratch unlink. Windows fixtures prove canonical eviction
is blocked while the owned temporary hard-link directory remains removable.
Sharing or retained-copy failure holds cleanup; no weaker fallback is used.

CLI success contains fixed `reel.fenced-job-summary.v1`, output count/logical
bytes, linked/copied input counts and boolean status only. Job IDs, paths,
hashes, roles and authority data remain in private library results, controls
and stderr. Success reports retention and `retirement_completed`, no daemon and
no authority for canonical cache removal. It does not claim zero copying or
zero scratch removal.

## Adoption and validation

Enrollment is opt-in in the admitted output-cache domain; inputs may come from
other active caches. Complete consumer policy, cross-domain routing/lock order
and final demand/custody checks for canonical cache unlink remain required.
Legacy jobs are not enrolled automatically. Existing cache removal does not
automatically honor new controls. Installed bindings and live caches are unchanged.

Interruption after registration but before unlink leaves collectible intent and
permits explicit reservation reconciliation and verified discard without tool
replay. Partial publication or partial filesystem unlink can still require manual
reconciliation; there is no automatic repair service or power-loss guarantee.

Seven Windows native-runner cases cover typed demand, summary privacy,
input/output contention, missing inputs, invalid controls/domains, explicit
retention/control budgets and discard refusal. Three executable cases cover
dispatch/count-only output, rejected options/missing domains and parent
termination with descendant exit/durable demand. They compile a tiny synthetic
Rust tool in temporary directories; no production cache, private media, external
service or installed runtime is accessed.
Automatic-retirement coverage adds success/failure salvage, artifact-fence
refusal, a simulated pre-unlink fault, and a real killed commit writer followed
by explicit reconciliation/discard. Missing custody and corrupt intent hold
cleanup. Two additional executable cases cover automatic success and failed-job
salvage, with no remaining scratch and count-only success output. A subprocess
harness is included in the test count; interruption during partial filesystem
unlink is not established by the pre-unlink process test.
