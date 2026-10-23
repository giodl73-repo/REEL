# Scoped native-language authoring review

Base: main `e15fedc`. Scope: explicit scoped project intake, native candidate
application and session metadata/UI. Producer-applied Editor, Animation Director
and Rights/Provenance checklists; no independent or human creative approval.

| Finding | Correction and actual evidence |
| --- | --- |
| Removing a language to fit the old editor would lose authored context | Project v3 explicitly chooses one language while capturing every lane's existing native alignment index and controls. Session/edit v4 binds that language plus descriptor and full input hashes. Both Spanish-first and English-first fixture edits reopen with the other lane intact. |
| Native alignment indexes are per-language, not one merged cue manifest | Routes name separate native indexes, each resolving filenames relative to itself and exactly covering its lane's cues. Unambiguous language/cue role pairs permit overlapping local cue IDs. Duplicate/aliased/escaping index routes and incomplete scopes reject. No alternate scene grammar or discovery is added. |
| Shared defaults and rows must not retarget another language | Scoped substitutions use native picture overrides; shared defaults stay intact. Reorder changes local event bindings. Removing a shared row narrows language applicability and keeps its other realizations. Cross-language shared slots refuse retargeting; their other references keep them retained during removal. |
| Preserving decoded JSON alone could invalidate untouched alignment bindings | Snapshot raw buffers preserve exact unchanged bytes, including pretty formatting/newlines, without reopening source paths during apply. Hash/size checks refuse tampered snapshot buffers. Native original-buffer parsing also rejects duplicated typed fields before Value normalization hides them. Existing budgets remain bounded. |
| A candidate must preserve the protected lane's actual selected closure | Before output, checks compare materialized other lanes, raw local bindings, alignments, unrelated asset bindings, normalized graph events/slots and active node references. Protected graph record spellings are restored after native serialization; the complete candidate reopens through full-scene intake. All sources remain unchanged and outputs are create-new. |
| Metadata success needs an actual native compiler round trip | A bilingual two-cue fixture applies reorder, off-frame holds, picture substitution and row removal, then compiles both languages using their separate native indexes. The edited language has three events; the protected lane retains four. Exact protected alignment bytes and selected graph records survive. No rendering/media acceptance is claimed. |
| Export must not hydrate a protected lane just to edit another | A synthetic scoped export succeeds with only the target images/movie present; other-language media is deliberately absent. Session v4 names the target and excludes foreign picture choices. UI tests bind the selected language, reject downgrade/scope changes and preserve scoped comments through actual handler-driven downloads. |

Windows validation: all 310 asset/assembly Rust tests plus three native compiler
integration cases pass (313 total), along with scoped strict Clippy, formatting,
ROLES and diff checks. All 32 asset/editor JavaScript tests pass, including
18 cel logic/handler cases. The existing Unix-only path fixture remains CI work.

Private regression: existing single-language mapped identity/reorder/removal
requests pass metadata-only (40/40/39 events), with all thirteen original control
hashes unchanged. These private scratch results remain outside the public repo.
They do not establish private multilingual episode or full-render acceptance.

Limitations: one explicit edited lane, one scene node and a complete selected
graph covering that scene's native languages. Separate per-language graph-file
families need explicit adapter work; they are not silently combined. Shared slot
forks, new asset insertion and simultaneous linked edits remain unsupported.
The producer supplies a preview movie matching the selected language/order;
its audio is not certified by metadata checks. Whole-scene/graph hashes change,
so older build receipts remain attached to their original snapshots. Browser
visual acceptance, a real private multilingual family fixture and runtime
adoption remain open. No live cache changes, production media access, server,
daemon, new cache, eviction or shared runtime replacement occurred.
