# Native workflow journal completion — producer handoff

REEL's isolated workflow branch now connects real native input claims, response
ingress and retained-owner verification to the existing append-only task journal.
The new CLI entries are `create-handoff`, `import-handoff` and `resume-handoff`.
Import durably records original result intent before effects. Only actual
retained-owner work/coverage and exact reopened output bytes permit a `Retained`
event and technical `task_completed:true`. Selection and creative approval remain
separate. Direct/manual provider results are supported in this bounded slice.

The private profile pins original admission/retained target in the ticket's native
binding and requires the Tool hash/length to match the executable actually running
the adapter. A different named executable cannot satisfy the pin. Original task,
attempt, native work/activation/checkpoints and retained ownership are preserved;
no replacement identity is allocated. Existing frozen tools must remain available
for their tickets; upgrades require explicit migration. Machine locators may
point at an exact relocated executable with identical bytes.

Reopening is passive, including original-generation recovery while native Prepare
is pending. It grants no permission to start new mutations. Effect entrypoints
reacquire/recheck native generation and reconcile exact original SDK work. A lost
completion marker reuses native publication, retained work and the original
canonical acknowledgement without requiring return files. Completed-journal
resume rechecks actual native proof; local state and saved CLI success are not
authority. Partial/corrupt proof and event tails remain and stop recovery.

The integration fixture exposed a retained-admission gap: native byte read guards
also hold object gates needed by a NEW index update. The adapter now closes those
read handles before admission, holds original durable job protection and ordered
source/retained fences across the transition, checks that original response
coverage is still present, and reopens bytes after retained admission. Lost source
coverage also blocks retrying merely staged retained work. Positive proof remains
held through the final journal append. Existing retained-Prepare recovery tests
had not exercised this new-admission gate conflict.

Acknowledgements are bounded private controls, exclusive-created, flushed and
reopened. Existing controls must match original candidate/work/evidence/target
and pass native verification; new checkpoint witnesses never overwrite them.
Their exact canonical artifact is pinned in the journal. Cooperating journal and
acknowledgement writers reuse existing metadata locks. Active directory/file/tool
guards protect each call; days-long waits rely on durable claims, not OS handles.
No expiry or cancellation releases assets. These controls are not a new cache.

Bounds: Windows/NTFS and existing explicit controller/store scope; at most 128
acknowledgement files of 64 KiB each; journal 128 events/1 MiB per event; existing
32-output/128 MiB, native resource, 16-total-actor and 128-changed-path phase bounds.
Controls must stay outside pool/index/metadata/coordination roots and not overlap
the journal. Initial uncertainty before a complete Waiting/acknowledgement record
requires explicit diagnosis; no truncation, overwrite, adoption or deletion is
inferred. Unknown native I/O stays NeedsRecovery; structured phase codes identify
reopen, ingestion and retention boundaries without exposing private paths/text.

Default fixtures cover actual runtime/target/control binding, absent providers,
preserved pending intent/claims and lost original response coverage. An explicit
native fixture combines response Prepare recovery, publication, NEW retained
admission, byte reopening and journal completion across CLI processes. It tests
lost completion recovery without return files, a busy former source actor and
corrupt acknowledgement refusal. Inputs and outputs use only the existing
synthetic test pool. No pool setup, hydration, external provider call, project
profile change, payload deletion or project selection occurs.

Validation: 33 shared workflow tests, 33 default native tests and five explicitly
invoked native fixtures pass (71 total). Strict Clippy for both workflow graphs,
root/native fmt and diff checks pass locally. All 249 original SDK/lifecycle
source pins remain exact. Hosted CI has not run. Qualification evidence, source
manifests and frozen executable profiles remain outside Git.

Next: qualify deterministic comic worker receipts and source correspondence in
this lifecycle. Expected-baseline selection, guarded actor close and scratch/
capacity retirement remain separate. API/proxy and ElevenLabs providers, real
project adoption and public release remain open. Technical task completion grants
no creative, translation, likeness/voice, rights or printer approval.
