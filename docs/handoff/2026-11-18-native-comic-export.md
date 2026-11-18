# Native comic export and editable recovery

The native workflow has explicit `prepare-comic-export` and `export-comic`
commands and a Rust `comic_export` API. They materialize a complete producer
packet from the original retained native source/output objects, with exact Git
commit/record/annex binding and retained work/owner/store coverage rechecked.
Captured bytes come from the same deterministic replay that guards retention;
ordinary retained verification does not retain extra packet buffers in memory.

Export uses existing owned staging/capacity machinery, not another cache or
storage publisher. Preparation makes a fresh owned intent and conservative
reservation only after retained proof passes. Persist and externally pin the
private ticket before exporting. Export reopens its exact namespace identities,
original reservation and original acknowledgement. Positive retained coverage,
native read guards and tool/snapshot pins stay live through file publication and
completion verification. Existing files are compared, flushed immutable parts
can reconcile, and corrupt files or partial evidence are preserved. Input staging
keeps its original ticket and completion serialization; export has separate
versioned schemas and operation identity.
The export identity uses original admission/work identity, not refreshable
checkpoint witnesses. A serialization regression verifies stable identity after
witness refresh and a changed identity for different native work; it grants no
custody. The exact original ticket/intent still pins all control bytes.

The owned workspace contains workflow controls and a `packet/` subtree with the
exact producer receipt, all named outputs and editable `sources/` copies. That
subtree is a normal comic packet. `comic-restore` checks its externally pinned
receipt and source hashes before writing a new editable source folder. It does
not independently verify native custody. Local source restoration permits an
exact rebuild and a fresh lettering version; it does not select that version.

The extended existing-store fixture removes both original raw source and rendered
worker folders, uses bare Git history and exercises CLI preparation/export,
wrong-ticket and wrong-owner refusal, completed export replay, missing native
store refusal, preserved corrupt local lettering, pinned editable restoration,
exact rebuilding and a fresh text edit with clean art unchanged. All 35 default
native tests and six explicitly invoked existing-store fixtures pass on the final
build, including the checkpoint-independent identity regression. The extended
group took about ten minutes. Strict native Clippy, formatting and diff checks
pass. The prior 50 shared workflow/publication/print tests and strict
Clippy remain applicable: all 24 shared source/manifest/lockfile entries match
their qualified committed manifest exactly. Shared tests were not rerun for this
native-only change. Hosted CI has not run. Synthetic controls, captured packets,
source manifests and the frozen native executable are preserved outside Git.

The copied packet and completion marker never replace native proof on another
export call. Capacity remains conservatively reserved, and no job claim, actor
slot, selection or scratch is released. Existing qualified Windows/NTFS, single
native byte-store, renderer, 256-file staging, 512 MiB reservation and bounded
attempt/entry limits remain. Files and controls are private; copying font/license
bytes is not a grant of distribution rights.

Native issue scheduling/upstream retained task inputs, complete issue retention,
deterministic dispatch, explicit selection and guarded retirement remain open in
the [completion ledger](../workflow-completion.md). This fixture does not adopt
BBJ/Melena, certify print or accessibility targets, grant creative/translation/font
rights, or authorize release.
