# Selected presentation source-text checks

Scene contexts can set `verify_presentation_sources: true`. Only native selected
source-text bindings are opened as metadata, through active-cache authorization,
hash/size, bounds and final snapshot checks. See [the contract](../asset-references-v1.md).

The inspector and rendering CLI share scene-to-invocation and source-scope
projection functions in `reel-assembly`; existing native wording/stanza/display
and native-line ownership checks enforce consistency. The bound authority record
must declare the same original source-document hash and scopes. Native resolution
and per-language source results stay separate; a failed requested source check
holds the context. Draft translation state is preserved. Reports expose counts,
hashes and states, not source wording or credits.

This does not extract/verify original manuscript content, decode the performance,
validate alignment timing/template definitions or supply consent/approval. Native
complete presentation coverage and rendered timing remain separate. No media
mutation, archive access, daemon, new cache, recursive discovery or global lock.

Review and actual validation: [producer review](../reviews/2026-10-21-presentation-source-checks.md).
The private consumer keeps the profile, existing source selections and retained
reports/runtime/evidence. Old profiles default to symbolic-only mode and old
saved reports remain readable. Next: native alignment metadata and timing checks
within the same explicit inspection scope; scoped decisions remain human evidence.
