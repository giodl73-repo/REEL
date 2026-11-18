# Owned deterministic comic dispatch

The native workflow adapter can execute one explicitly selected protected comic
task from a pinned ready batch. The supported implementation is the existing
Rust comic producer hosted in the exact lifecycle executable named by the task.
It supports whole editions and selected issue parts. This is a bounded worker
adapter; it does not introduce another storage engine, a provider transport or
an arbitrary process launcher.

`prepare-worker` verifies the current plan/batch/original task, actual runtime,
source claims and native input closure, then reserves an owned staging workspace.
The new private ticket has an explicit external byte pin. Preparation uses the
existing conservative capacity and immutable ownership/intent mechanism. A
partial control or interrupted reservation remains for recovery.

`materialize-worker` builds the editable packet in-process and writes invocation,
result, returns and inventory controls through exclusive bounded staging.
Materialization leaves an initially Waiting task incomplete. `run-worker` can
perform this step itself, then verifies the owned packet and delegates the generated tool
result to the existing native import/retention path. A retained result must be
reopened and acknowledged before durable task completion. Source object read
gates drop before new lifecycle effects; positive source claims remain.

Resumption preserves the original task/attempt/admission and operation identity.
Partial deterministic writes reconcile the same files. Pending or retained effects
reuse the completed owned packet and original result instead of issuing a new
attempt. Corrupt/replaced/unknown output, stale controls and mismatched executable
pins refuse without overwriting evidence. A local manifest or complete marker is
only a bounded workspace check; native retention and full comic replay remain
the completion authority. Actor limits, changed-path limits and native source
qualification are unchanged. Scratch/capacity retirement and selection remain
separate transactions. Passing renderer limits is not a reservation for every
future distinct result object; native result admission still enforces the
qualified close bound. Larger editions require explicit bounded parts, and a
refusal preserves materialized output and original claims.

Producer-applied role review uses the comics editor, story direction, editor,
platform/audience and rights/provenance checklists. The worker preserves exact
master/page/panel/utterance/language identity and editable art/lettering/font
sources through the existing producer. It does not invent translation, modify
canon, execute returned code, upload assets or infer font rights. Reading proofs
and accessible text remain distinct outputs. Neighboring-page/full-issue creative
review, printer proof and release still require the producing project's actual
authorities. These are checklist findings, not independent approvals.

Qualification: 60 shared tests, 36 default native tests and all nine explicit
existing-store cases pass (105 distinct tests). The new case dispatches one
selected part of a two-page master, compares the materialized packet with the
existing producer, leaves materialization Waiting, interrupts import on missing
returned files, recovers the original operation through the worker, retries the
retained result and refuses altered owned output without overwriting it. It then
exercises bare history, native editable export, restoration and a fresh lettering
edit. The unselected page remains unprotected. Missing staging bindings and wrong
ticket pins refuse before dispatch.

Both dependency graphs pass strict Clippy, formatting and diff checks. An initial
native doc-test hit a missing build artifact during overlapping Cargo invocations;
the serial native run, including doc-tests, passes. The final inspector also
verifies the earlier worker run through its frozen older runtime and bare history.
An initial narrower worker run passed separately; final qualification includes
the added staging-binding and interrupted-import checks. Frozen tools, synthetic
controls, logs and source manifests remain outside Git. Hosted CI has not run.

Real project adoption, larger native issue assembly with retained upstream inputs,
film producers, providers, issue/fix routing and admission/close scheduling remain
in the [full completion ledger](../workflow-completion.md).
