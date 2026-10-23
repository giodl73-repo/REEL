# Opt-in native job publication enrollment

`reel-assets --root <project> run-job-fenced <manifest.json> <existing-fence-directory>`
executes an explicitly requested trusted native job with durable input/output
demand and short per-asset publication fences. This is an execution command,
not a preflight or sandbox. Startup does not execute, resume, render or hydrate.

The manifest must explicitly set `retain_workspace: true`. Automatic retirement
is rejected: its diagnostics, salvage and evidence publications still need
enrollment. Workspaces remain bounded and retained on success/failure until
separately authorized verified discard. There is no new expiry, daemon or replay.
The existing `run-job` lifecycle is not switched.

The domain must already exist in a separate metadata child of the cache chosen
by capacity admission. A different admitted cache is not silently given another
domain. No cache, control or archive fallback is initialized. Canonical inputs/
CAS tools must be registered; installed runtimes remain separately pinned
external installations, not inferred CAS demand.

Intake reads at most 256 KiB and parses the exact bytes it records/hashes, with
UTF-8 BOM handling. This also removes ordinary intake's second-read race.
Enrollment permits 512 distinct input/tool hashes and bounded output roles.
Root `job-publication.json` and `job-output-intent.json` paths are reserved for
enrolled jobs.

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

Input-fence refusal precedes workspace creation and input payload access. Later
failures retain demand/workspace. Output-fence refusal leaves local output
unpublished to CAS. Explicit reinvocation creates a new job, never a hidden retry.
No scratch crawl infers undeclared outputs. Fences do not span execution, media
hashing or copying.

The native payload/scratch budget additionally covers manifest bytes and three
64 KiB control allowances. Enrollment, output intent and result each have that
individual limit. This is logical allowance, not exact physical allocation.
Existing native execution/output-count/timeout limits remain.

## Collection, discard and privacy

The native-job collector verifies control scope/manifest/domain, meters/stamps
controls and keeps input protection separate from output demand. Output intent
holds before CAS publication/result; it is not custody or a build-input edge.
Missing/recreated/corrupt controls and enrolled results without matching intent
fail collection rather than producing zero references.

Exact registered-manifest discard validates enrollment/domain before removing
an enrolled workspace. Existing admission, registration, retained-original/output
verification and explicit incomplete-discard checks remain. No canonical cache
deletion or automatic candidate registration is added.

CLI success contains fixed `reel.fenced-job-summary.v1`, output count/logical
bytes, linked/copied input counts and boolean status only. Job IDs, paths,
hashes, roles and authority data remain in private library results, controls
and stderr. Success denies removal authority and reports retention/no daemon;
it does not claim zero payload copying.

## Adoption and validation

Enrollment is opt-in in the admitted output-cache domain; inputs may come from
other active caches. Complete consumer policy, cross-domain routing/lock order
and final demand/custody checks at unlink remain required. Legacy jobs and
auto-retirement/salvage are not enrolled here. Existing removal does not
automatically honor new controls. Installed bindings and live caches are unchanged.

Seven Windows native-runner cases cover typed demand, summary privacy,
input/output contention, missing inputs, invalid controls/domains, explicit
retention/control budgets and discard refusal. Three executable cases cover
dispatch/count-only output, rejected options/missing domains and parent
termination with descendant exit/durable demand. They compile a tiny synthetic
Rust tool in temporary directories; no production cache, private media, external
service or installed runtime is accessed.
