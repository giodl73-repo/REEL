# Producer-derived native review exports

`retention::review_copy` now derives review controls while holding complete native
retained-result and producer proof. For FilmRender this includes the existing
independent media/primary comparison. For ComicRender it includes the existing
deterministic source/output replay. A serialized acknowledgement or caller-authored
map cannot construct this guard. Historical export uses original task/runtime
pins and retained coverage, without requiring the original source actor generation.

The film adapter reads the verified `review-ids.json`, preserves attachment IDs,
cel/shot IDs and half-open frame intervals, and pins the exact review MP4 and job.
Shot-only targets have their own tagged type. Optional shot identity on cel
targets is omitted from legacy JSON when absent. The adapter does not infer cue
identity from picture attachments or allocate new on-screen IDs. The original
retained label map still carries the sample clock and selected font lineage.

The comic adapter preserves selected page order and page/panel/utterance IDs from
exact source binding. It exports a small reading manifest referencing every
ordered reading PNG. The review copy pins that manifest; hashed lookup keys
address the original tuples rather than allocate new panel or balloon IDs.
Selected-part exports describe only their selected pages. Whole issue assembly
review export remains separate from ComicRender/part export.

```text
reel-workflow-native export-review-copy PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RESULT ACK NEW_COPY [NEW_COMIC_MANIFEST] --apply
```

Film export takes no manifest argument; comic export requires it. Controls are
private, bounded and exclusively created. Write failure preserves partial files;
no automatic replacement, scratch retirement, selection or claim release occurs.
Exported derived controls are not retained merely because their source media is
retained. A later workflow task must explicitly admit them. Export does not prove
watching/listening or authorize publication.

Qualification is pending. The ignored existing-pool fixture takes explicit
retained movie/comic roots and writes its operation marker before exporting.
It checks the 48-frame film cut at frame 24, actual cel/shot targets, ordered comic
reading pins, finding binding and unchanged claims. It does not render new media.

Producer-applied editor, comics editor and rights/provenance review: mappings use
producer IDs and selected versions, and local repairs retain the full contextual
review requirement. Technical map/media checks grant no creative, canon,
translation, font/voice, selection, print or release authority. No independent
review is claimed. Structured native review ingestion, authorized issue delivery,
repair execution and evidenced closure remain open.
