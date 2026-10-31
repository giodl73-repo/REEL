# Historical cleanup does not require an obsolete external installation

The runner materializes only cached inputs and cached tools into owned scratch.
Installed tools execute at their admitted external paths. Requiring an old
external executable to remain installed and match its former hash made historical
scratch cleanup depend on unrelated tool installation state. It could also be
mistaken for current production revalidation, although cleanup runs no tools.

Retirement now checks those installed paths against the canonical removal scope.
Missing leaves are resolved through bounded surviving ancestors; an installed
path or junction into scratch still refuses. Ambiguous/unresolvable namespaces
fail closed. The command neither reads/executes/replaces external executables nor
claims they remain available or match old producer bytes. Scope exclusions are
reported separately from verified retained-object counts.

All cached inputs/tools, request, evidence and output candidates remain hash
verified under mutation/deletion guards. Source/result/operation admission,
manifest/output correspondence, exact-job lease checks, snapshot identities,
transaction integrity and explicit apply remain unchanged. Fixture cases cover
missing installations and parent directories, upgraded external bytes, a tool
inside scratch and a junction that makes an external path resolve inside scratch.

Producer-applied storage/provenance checklist: only the named workspace is
removed; every materialized original has a guarded retained copy; external
runtime changes are not reinterpreted as new producer claims or approval. This
is a technical checklist, not independent approval. Actual consumer acceptance
and incomplete/unpublished workspace salvage remain open.

Hosted qualification also exposed runner storage exhaustion while building
debug artifacts. CI now omits dev/test debug symbols and uses a fresh Rust cache
prefix, matching local qualification. The complete test matrix and assertions
remain enabled; no production cache or payload is removed to make room.
