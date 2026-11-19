# Auxiliary executable identities and original worker bindings

Deterministic `ToolContract` and expansion templates support optional
`auxiliary_executables`: role -> exact Artifact. These portable identities enter
the task key, native candidate context and resource tree at `runtime/aux/ROLE`.
Expansion preserves the declarations. They are runtime resources, not additional
creative inputs or instructions to execute arbitrary code.

Dispatch config separately supplies optional `auxiliary_programs`: role -> native
`Tool` (`path`, `sha256`, `bytes`). The set must match the task exactly. Missing,
extra, changed-hash or changed-length entries refuse. Machine paths are frozen
in the worker ticket and invocation, and participate in the owned workspace's
operation identity. They do not enter the portable executable Artifact.

Worker preparation and execution reopen exact installed program files and retain
their read-only file and ancestor-directory guards through the synchronous
operation. No role authorizes an arbitrary command or shell invocation. A producer
must explicitly implement how it uses each supported runtime. These declarations
do not themselves authenticate execution, retain installed binaries in native
storage or qualify neighboring DLLs/environment.

Existing `prepare`/`prepare_file` APIs remain wrappers for an empty runtime set.
The new `prepare_with_programs`/`prepare_file_with_programs` APIs support explicit
bindings; dispatch supplies them automatically. `run-worker` consumes the original
pinned ticket, revalidates its program files and uses its frozen machine mapping.
Changed paths cannot retarget an existing owned workspace, even for identical
program bytes. Missing/corrupt programs require the original-work recovery path;
there is no PATH/latest fallback or automatic replacement attempt.

Empty maps are omitted from tool, template, config and ticket serialization.
Legacy worker invocation and operation fields remain unchanged when no auxiliary
programs are declared. Bounds: at most 16 lowercase bounded role names, 256 MiB
per executable descriptor, absolute machine paths of at most 4,096 encoded bytes
and 128 components, and existing 16-actor/128-changed-path admission limits.
No limit is raised or automatically split to accommodate a larger runtime set.

The worker holds installed bytes only while running. Durable job claims protect
the declared object identities in their assigned native stores; they do not
prevent an operator from changing an unrelated installed executable after exit.
Subsequent execution must reopen the exact original bytes or refuse. Releasing
claims, selecting candidates and retiring scratch remain separate operations.

Component tests cover task-key invalidation, empty-map compatibility, expansion,
role/pin mismatch refusal without opening paths and unchanged intelligence/provider
contracts. The native qualification extends the existing selected-take scene
control fixture: explicit installed tool binding, native resource-tree membership,
retained outputs/replay, same-length executable corruption, same-byte path
retargeting, unchanged claims/ticket on refusal and restoration under the original
ticket. The auxiliary Git copy is declared but not executed by that pure control
producer; this fixture does not claim movie rendering or media-tool execution.
Actual qualification evidence is recorded after running it.

Producer-applied custody/recovery and rights/provenance checks keep runtime
identity, source authority, private machine bindings, retained output coverage and
creative/release approval distinct. This is a prerequisite for the owned movie
producer, not completion of that producer or independent review.
