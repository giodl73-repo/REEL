# W2 existing-scene inspection review

Author-applied inspection under [REEL's role rules](../../.roles/ROLE.md), not
independent review or production approval. Scope: pinned native v2 metadata and
private sidecar export. Fixtures are synthetic; no project media is opened.

| Lens | Finding and disposition |
| --- | --- |
| [Story](../../.roles/story-director.md) | Drawing-derived IDs would lose repair targets on redraw. Require supplied story/cel identities and agreement for shared semantics across languages; retain the cue spine and episode/scene order. |
| [Animation](../../.roles/animation-director.md) | Pacing/style fields could imply renderer support. Preserve rational desired cel duration, school, performance style, technique and pinned renderer/settings; leave renderer qualification false. |
| [Editor](../../.roles/editor.md) | Inspection could trim breaths or omit full presentation. Reuse cue-opening rules and preserve presentation/source references; whole-episode conform/viewing remain later gates. |
| [Sound](../../.roles/sound-designer.md) | Cross-language clock reuse can mis-time a selected take. Check each language's exact text, word evidence and selected alignment against its own take hash, clock and markers. Read no audio; claim no acoustic measurement. |
| [Lyrics/Vocal Adaptation](../../.roles/lyrics-vocal-adaptation-editor.md) | Timing could be mistaken for performed-text or translation approval. Preserve text/authority layers, reject mismatches and perform no rewriting, transcription, recording or listening approval. |
| [Rights/Provenance](../../.roles/rights-provenance-steward.md) | A redraw could inherit approval or inspection could hydrate private inputs. Preserve exact historical subjects without promotion. Read only named local committed controls, reject partial clones, disable replace refs and fail on missing history. Reports omit private identities. |
| [Platform/Audience](../../.roles/platform-audience.md) | A sidecar might be advertised as a usable film. Keep media, renderer, approval and production-adoption flags false. Captions, credits, editions and project-scale acceptance remain later gates. |

Retained limitation: this Git reader handles committed ordinary controls, not
record/annex payloads or arbitrary old scripts. W3a must use qualified exact
record-to-leaf resolution, without silently changing accepted producers' SDK
pins. This PR adds no new media producer or comic dependency.
