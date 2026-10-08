# Ordered episode presentation

`reel-episode-conform` also accepts `reel.episode-master-template.v2` with an
`ordered_units` array. Each unit names one `scene` or `episode-presentation`
segment by its stable ID. The order is exact: a display can precede, follow,
or sit between scenes, and two displays can be adjacent. The conform manifest
still uses `reel.episode-conform.v1`; its ordered segments must match every
template unit exactly. The selected template is pinned by the catalog hash,
and an optional upstream source template must contain the same ordered units.

For an editable display, the unit carries `source_ids` and aligned SHA-256
arrays under both `es` and `en` in `source_text_sha256_by_language`. A source ID
may appear in only one display unit. The language-local segment supplies a
`source_text_evidence` file reference with schema
`reel.presentation-source-text.v1`. It records episode, role, language, each
source ID, editable UTF-8 text and its hash, the caption template ID, and a
separate clean-picture binding and hash. The episode presentation invocation
pins this evidence hash in its content; its selected presentation receipt pins
the same hash as `selection_evidence_sha256`. The binding must resolve once in
the selected season or episode asset bindings and have a cache URI, byte count,
matching SHA-256, and selected production state. The selected presentation
master remains separately bound and decoded by the existing conform checks.

Repeated presentations may share a template ID and layout. Give each ordered
unit and presentation invocation a unique occurrence role, and set the
invocation's `template_kind` to the shared kind (for example, `poem-title`). The
adoption manifest uses that same occurrence role and `template_kind`. Bind each
occurrence's master, picture and source-text evidence separately. The source
evidence role must match its occurrence, not the shared kind. Existing manifests
that omit `template_kind` keep their original role-as-kind behavior and hashes.

This is a selection-time contract. Source-draft `scene.json` files and episode
contexts can be prepared without a selected picture, caption render or master.
They cannot pass full episode conform until the presentation and scene inputs
are ready. The conform verifies the declared picture binding metadata; it
does not inspect that picture's pixels or prove its appearance in a rendered
master. Picture-to-master visual review, title wording, attribution, sound,
principal approval and publication remain downstream checks.

The v1 master template and its contiguous `chapter-scenes` role remain
supported. The bilingual synthetic contract test covers adjacent editable
displays, order and coverage rejection, and wrong-language evidence.
