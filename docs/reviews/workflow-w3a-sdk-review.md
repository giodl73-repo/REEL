# W3a.1 SDK source intake review

Author-applied inspection under [REEL's role rules](../../.roles/ROLE.md), not
independent review or production approval. Scope: dependency source, manifests,
licenses, isolated compilation and byte/set verification. No production effects.

| Lens | Finding and disposition |
| --- | --- |
| [Rights/Provenance](../../.roles/rights-provenance-steward.md) | A source ancestry commit could conceal qualified working-tree changes. Preserve the exact original manifest and all 247 file pins; preserve the two lifecycle module/test pins and explicit qualification digest. Reject unlisted source files. |
| [Rights/Provenance](../../.roles/rights-provenance-steward.md) | The capacity manifest omitted its upstream license. Include the unchanged upstream MIT license separately and pin its bytes. Verify all 379 source entries against the clean upstream source commit. |
| [Rights/Provenance](../../.roles/rights-provenance-steward.md) | A new SDK could change accepted producers or admitted job runtimes implicitly. Use a distinct workspace/lock and namespaces; leave the root producer SDK, lock, profiles and active operations unchanged. |
| [Platform/Audience](../../.roles/platform-audience.md) | Successful SDK compilation could be described as the usable film system. Expose no effectful runtime command in this slice. Document source intake versus consumer lifecycle, production-scale acceptance and project adoption. |

Retained tension: two SDK versions add source volume but preserve the qualified
consumer boundaries. This is the explicitly separated source-intake portion of
W3a, not a new cache engine or an invitation to change metadata limits. The
subsequent adapter must qualify exact input binding, original-operation recovery,
retained readback and guarded close before claiming production behavior.
