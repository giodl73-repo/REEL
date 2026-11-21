# Existing-film inspection sidecars (W2)

`reel-workflow inspect-film REPOSITORY REQUEST_JSON NEW_SIDECAR_JSON` reads
explicitly named controls from one exact local Git commit and writes a new
private sidecar. It does not check out that commit, change the working tree,
index or profile, read selected media, invoke a producer or call a model.
Existing production continues through its accepted entry point.

The initial adapter supports `reel.scene-authoring.v2` with selected single-take
cues and committed ordinary JSON/text controls. It reuses `reel-assembly`'s
materialization, selection and text-trigger rules. Arbitrary legacy scripts,
segmented takes, raw `.tig` records and annex-backed controls are not interpreted
by this Git reader. Native record-to-annex resolution belongs in W3a; this adapter
does not replace that binding with a search for matching bytes. No existing
project has been adopted through this metadata-only interface.

## Request

The strict `reel.workflow-film-inspection.v1` request contains:

| Field | Meaning |
| --- | --- |
| `project`, `source_commit` | Project identifier and full exact commit ID. Branch names, tree IDs and latest fallbacks refuse. |
| `episode`, `scene`, `catalog`, `policy` | Exact committed native authoring controls. |
| `scopes` | Three controls in season, episode, scene order, with matching scope IDs. |
| `cues` | Requested language -> cue ID -> `{text, words, selected_alignment}`. Text is `text/plain`; words and alignment use the existing native JSON schemas. |
| `identities` | Requested language -> event ID -> `{story_id, cel_id}`. Supply existing project identities. Shared semantic events must keep the same identity across requested languages. |
| `direction` | `{desired_seconds_per_cel, visual_school, performance_style, animation_technique, renderer, settings}`. Duration is a positive `[numerator, denominator]` ratio. Renderer is an artifact; settings are a pinned JSON control, including existing HTML/CSS/SVG settings where applicable. These are declared choices, not renderer qualification. |
| `approvals` | Existing `{subject, evidence}` associations. Subject is the exact original artifact and evidence a pinned control. Historical subjects remain historical after a redraw; authority and approval decisions are not inferred or authenticated. |

Each control has this shape (the hash placeholder is explanatory):

```json
{
  "path": "scenes/scene-1.json",
  "artifact": {
    "logical_id": "controls/scene-1",
    "sha256": "<exact SHA256 of committed bytes>",
    "bytes": 1234,
    "media_type": "application/json"
  }
}
```

Paths are portable relative paths with ASCII alphanumeric, dot, underscore and
hyphen components. Absolute paths, traversal, symlink/submodule modes, mismatched
pins and conflicting path/identity aliases refuse. A complete local repository
is required; promisor/partial-clone configurations refuse before control reads.
Replace refs are disabled. Missing history is not fetched or substituted.

Each requested language must provide exactly its native cue and event set. Its
selected-take hash, word timing, exact source text and selected alignment must
agree. The derived alignment must match the selected alignment's clock/markers.
One language cannot fill another's missing evidence. Established cue-opening
rules cover leading recorded breath/silence. Inspection neither trims that
silence nor listens to or measures audio. These clocks are cross-checked metadata,
not new acoustic measurements. It preserves original text/authority layers and
does not translate or revise them.

## Result and limits

The sidecar preserves request/control pins, episode ordering and presentation
references, the complete authored scene, story/cel/cue identities, language-local
alignments, selected picture/take versions, score/VFX/sonic uses and per-language
selected input metadata. It preserves presentation, score and approval references
without asserting their full production closure or validity. It makes no captions,
credits or delivery editions.

`metadata_verified` is true for these scoped checks. `media_verified`,
`approvals_verified`, `renderer_qualified` and `production_adopted` remain false.
CLI output contains generic status/error codes. Sidecar/request contents are
private and belong in project-owned storage outside the public source repository.

Bounds: 256 control paths, 1 MiB per control, 16 MiB aggregate control bytes,
32 requested languages, 128 approval associations and 1 MiB final sidecar. Each
invocation covers one scene; larger episodes use bounded scene slices without
changing their identities/order. These metadata bounds are not universal media
limits. Output is created exclusively after checks; existing outputs refuse and
failed writes are preserved.

Run the synthetic committed bilingual fixture:

```text
cargo test -p reel-workflow --test film_inspect --locked
```

The tests create disposable local Git repositories and invoke both the API and
actual CLI. No media payloads exist there. They cover fixed snapshots, dirty-tree
isolation, language/take/clock checks and stable IDs across redraws with unchanged
earlier approval subjects. They do not qualify an actual project or episode scale.
W3a supplies qualified native storage/tool integration; W4 supplies review/issues
and affected rebuilds.
