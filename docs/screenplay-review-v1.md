# Pinned screenplay development review

`cargo build --locked -p reel-screenplay` builds a separate, provider-neutral
crate and CLI. It does not depend on Tig, generate audio, upload, or start a
server. The producing project owns show identity, source text, cue identity
maps, age/casting decisions, rights and delivery conventions.

```text
reel-screenplay export REQUEST SOURCE_ROOT NEW_DIRECTORY
reel-editor-host serve NEW_DIRECTORY --port 8876
```

Request schema `reel.screenplay-review.v1`: `show_id`, `title`, `revision`,
`source_commit`, `notice`, ordered `languages`, ordered `episodes`, and optional
`visual_roles` map. IDs used in filenames admit ASCII letters/digits/underscore/
hyphen. Episodes contain `id`, positive `season`/`number`, and one source for
each language. An episode may instead be `{ "path": "episodes/e1.json" }`,
resolved under the request directory with a 64-KiB per-definition bound.

Each source contains `language`, root-relative `path`, exact `sha256`/`bytes`,
and `cues`. A cue maps to `{id,name,generic}`. These are explicit project
casting/alias identities, not inferred person equivalence. Unnamed roles should
be scoped to an episode unless the project positively establishes recurrence.
Two shows may use the same episode IDs; the session's show ID is the namespace.
One packet contains one show; switching shows means opening its own packet.

Sources are captured against their pins, at most 1 MiB each and 32 MiB combined.
The format is a **bounded Fountain subset**, not a complete Fountain parser:
uppercase/forced cues, blank-ended dialogue, standalone parentheticals, common
English slugline prefixes and forced headings. Every initial scene unit needs
`#scene-id#`; secondary sluglines may continue the current unit. Numbered IDs
must be unique. Boneyards, notes, dual dialogue, unmapped cues and narrator/VO
cues refuse. Editorial prose, ambiguous all-capital action and unsupported
formatting need source review; a pass is not a complete language or narration
lint. This version does not parse arbitrary localized slugline conventions.

The analyzer checks scene-unit/slugline order and canonical speaking-turn order
between versions. That does not prove translation meaning, ages or timing.
Each turn records the one-based source line immediately after its cue, which
may be a parenthetical. Counted dialogue lines exclude standalone parentheticals.
Words are Unicode letter/number tokens with internal apostrophes/hyphens.

Exports use a new directory and create-new files. Later revisions never overwrite
an earlier packet. The packet includes the frozen session, bundled review HTML/JS,
the original request and one complete UTF-8 reading manuscript per language.
Reading manuscripts preserve each episode's original Fountain text and headers;
they are editable development text, not typeset production PDFs/DOCX. Partial
IO failures retain incomplete staging; absence of a completed session is not
permission to delete anything. Keep raw packets out of the engine repository.

The existing native editor host recognizes `reel.screenplay-session.v1`,
verifies the bundled frontend revision, and serves only the three frozen review
resources through its existing private loopback capability URL. Session JSON is
bounded to 2 MiB; manuscript/request files are not served. Export never starts
the listener. Existing host deadlines, GET/HEAD restrictions and path checks apply.

The browser provides one-language-at-a-time season/episode/character/role
filters, source-line/turn/word measures, character pivots across seasons/episodes/
overall, filtered word shares, episode totals and CSV export. It makes no
automatic lead/supporting assignment, narrator comparison or runtime estimate.
Silent roles can be acknowledged explicitly; speech counts do not measure
screen presence. Browser interaction and human casting/translation review are
separate from parser tests.
