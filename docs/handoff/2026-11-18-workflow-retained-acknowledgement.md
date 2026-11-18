# Retained workflow acknowledgement — producer handoff

The isolated REEL workflow branch now stages and executes workflow-specific
retained actor registration for already registered provider responses. The CLI
adds `stage-retention`, `retain-response` and read-only `verify-retained`.
Actual positive retained coverage and exact reopened native output bytes are
required before acknowledgement; a serialized receipt is not live proof.

The ready retained actor has an explicit enrolled owner, differs from the job and
routed source actors and covers every store assigned to the original job actor.
New admission holds source/retained fences in stable order. The retained update
keeps all existing paths and the prior root, adds every current job reference/root
and pins its evidence. It never closes the job actor, releases input protection,
retires scratch/capacity or changes a project selection.

Index/task/attempt identify the original operation. Evidence binds the response
registration and WorkRef, original admission and retained target/owner. Exact
retries reuse the same work; changed results/targets conflict. Interrupted native
Prepare can reconcile only that original admitted operation. Lost replies use
a new private receipt destination. Already completed work can regenerate an
acknowledgement after source actor advancement, with the same WorkRef/evidence
and fresh checkpoint witnesses. Historical verification holds only the retained
fence, checks current positive retained coverage and reopens the exact outputs;
missing bytes and lost retained claims stop without substitution or republication.

This is a bounded workflow adapter over existing SDK metadata/index/work, not a
new cache engine or the managed scene runner's JobContext/SourceJournal outcome
adoption. Vendored qualified SDK and lifecycle sources remain unchanged. Byte
reopening uses the existing explicitly declared single native store. Enrollment
must cover all other assigned stores; no provider discovery or adoption occurs.
Historical input claims/archives do not assert all input payloads are available.

Bounds: existing Windows/NTFS cooperating scope; 32 output artifacts/128 MiB output
bytes; 512 distinct retained resource objects/1 GiB summed logical resources;
one 64 KiB evidence node; 128 changed paths and existing tree limits. Oversized
updates stop without new actors, implicit phases or history deletion.

Two metadata fixtures exercise staged replay, wrong owner/actor/store scopes,
original native Prepare recovery and preserved positive job claims. One explicit
native fixture exercises byte reopening, interrupted retention and lost-response
replay across CLI processes, unrelated retained history, acknowledgement recovery
after source advance and refusal on lost coverage. Its deliberate synthetic
source advance and coverage loss/restoration do not qualify guarded slot close
or retirement. No payload is deleted or project selection changed.

All 30 default native tests and four explicitly invoked native fixtures pass.
Strict native Clippy, root/native fmt and diff checks pass locally; all 249
original SDK/lifecycle source pins match. Shared workflow/publication/capacity
sources are unchanged and hosted CI has not run. Persistent synthetic evidence,
committed-source manifests and frozen executable profiles stay outside Git.

The native `handoff::Lifecycle` implementation and durable coordinator completion
remain the next gate. Deterministic comic worker receipt/source correspondence,
expected-baseline selection, actor close and scratch/capacity retirement remain
separate. Acknowledgement grants no creative, translation, likeness/voice, print
or release approval. No production film/comic project has adopted this adapter.
