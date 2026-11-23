# Visual production board

A private, read-only board for film cels, drawing versions, selected recordings,
language-local semantic markers and observations from existing workflow journals.
The same unit model can label comic pages. Film is the primary qualification case.
It uses a pinned board manifest and existing local preview files. The film
projector builds that manifest through REEL's existing exact-commit inspector;
it does not discover a project or choose preview locations.

```powershell
cargo build --manifest-path crates/reel-workflow-ui/Cargo.toml --locked
reel-workflow-ui capabilities
reel-workflow-ui project-film REQUEST.json RAW_SHA256 REPOSITORY NEW_BOARD.json
reel-workflow-ui check BOARD.json RAW_SHA256
reel-workflow-ui serve BOARD.json RAW_SHA256 --open
```

The owned host is Windows-only; declaration validation is portable. `check` reads
metadata, not media. `serve` starts an explicit loopback listener with a random
capability URL. Opening the browser requires `--open`. Defaults are a 15-minute
idle timeout and six-hour maximum; `--idle-seconds` and `--max-seconds` are explicit
bounded settings. Closing the viewer does not cancel work, release protection or
retire scratch. Keep its URL and source manifest private.

## What the board shows

- Scene navigation, stable story/C IDs, selected/historical versions and visibly
  unselected candidates. Choosing a version in the viewer changes only the view.
- Language-specific cue text, the exact declared selected recording, playback,
  semantic marker seeking and cel highlighting on that recording's sample clock.
  Language switches use the new language's clock. No automatic playback.
- An inspector with exact asset identity/hash and recorded approval references.
  Approval authority is explicitly unverified.
- Live observations for specifically declared `(task, attempt, ticket)` journals.
  Returned image candidates appear on their declared unit without replacing its
  selected version. Incomplete/unavailable observations preserve the last view.

Journal state is observation, not proof of native custody. Even a journal's
`retained` state is displayed as **Retention recorded**, with
`native_custody_verified: false`. Native verification remains an explicit command
of the existing managed workflow. The board has no execution, retry, approval,
selection, issue-submission or publication endpoint.

## Manifest and preview boundaries

`model.rs` defines the strict `reel.workflow-board.v1` declaration. Each version
contains its artifact, designation, optional language and preview key. Each span
names a language-local cue and sample interval whose start matches the unit's
semantic marker. Media entries bind explicit absolute paths to exact artifacts;
an image derivative may declare its subject, without claiming verified derivation.
Cue previews must equal the selected take so a different proxy cannot silently
borrow its clock. Watch entries pin the canonical ticket digest and exact task and
attempt; each output binds an explicit path and target unit.

## Existing-film projection

`project-film` reads a private pinned `reel.workflow-film-board.v1` request and
passes its existing W2 inspection request through `film_inspect::inspect` against
the exact Git commit. It reads only admitted committed controls, including exact
source text and measured language-local alignment evidence. It does not inspect
the working tree's replacements or open referenced media. Partial clones and
missing historical metadata retain the inspector's refusal behavior.

| Request field | Meaning |
| --- | --- |
| `schema` | `reel.workflow-film-board.v1` |
| `inspection` | The existing [film inspection request](../../docs/workflow-film-inspection.md), including exact control artifacts, identities, cue evidence, direction and approvals |
| `title`, `scene_title` | Display labels; no source text replacement |
| `language_labels` | Optional language ID → display name map |
| `language_order` | Optional complete list of inspected languages; the first is the viewer default. Duplicates or missing lanes refuse. |
| `assets` | List of `{asset, preview}` bindings for every inspected selected picture/take. `asset` is an exact artifact; `preview` is an optional key into `media`. Matching bytes under a different logical ID do not qualify. |
| `media` | Explicit preview bindings in the board format above; files need not be available during metadata projection |
| `tasks` | Optional existing watches; each output's `unit_id` is the source shared event's semantic ID |

Unit order comes from shared scene events; story and C IDs come from the existing
inspection identities. Versions retain each language's exact selected subject.
Intervals end at the next cel in the same cue or that cue's end, preserving the
inspector's opening-breath rule. Source marker words and selected-take clocks are
never translated, regenerated or copied between languages. Ambiguous simultaneous
picture cuts refuse instead of choosing a cel arbitrarily.

All recorded approval subjects/evidence artifacts are preserved in `approvals`,
including multiple references and historical subjects. The inspector shows exact
and different-subject references separately; neither is verified creative authority.
The generated board also records the raw projection-request artifact pin.

Export validates everything before exclusively creating `NEW_BOARD.json`, prints
its hash/length and never overwrites an existing file. On an interrupted write,
the partial new file remains evidence; do not adopt it without checking its pin.
The export is an ordinary private control file, not native retained publication.
Each request projects one existing inspection scene slice; this does not qualify
complete episode assembly, project adoption or native previews.

Preview files open lazily, verify SHA-256/length in bounded chunks and support
GET/HEAD and byte ranges through shared `reel-assets::loopback` transport. Windows
ordinary-file/directory guards and retained read handles prevent namespace swaps.
No directory listing, native acquisition, hydration, upload or cache engine runs.
This local file provider does not establish native record-to-leaf custody.

Bounds: 8 MiB manifest, 32 languages, 256 scenes, 2,048 units/cues, 4,096 declared
media entries, 128 journal watches, 16 MiB journal observations per refresh,
16 concurrent requests and eight cached preview readers. Individual previews are
limited to 128 MiB and declared preview bytes to 64 GiB. These are **viewer** limits;
they do not redefine film/master limits or qualify episode-scale assembly.
PNG/JPEG/WebP, WAV/MP3 and MP4 routes are permitted; this board renders image
versions and audio cues. SVG/HTML payloads are not executed as media.

## Synthetic walkthrough

Choose a new absolute directory outside the repository. The example creates
synthetic PNG artwork, Spanish/English test tones with different clocks, a board,
and a fake workflow lifecycle. It makes no provider calls and proves no native
retention. Its printed SHA pins the raw board bytes.

```powershell
cargo run --manifest-path crates/reel-workflow-ui/Cargo.toml --example synthetic --locked -- create F:/YOUR-OWN-NEW-SAMPLE
# Use the printed board path and SHA with serve above.
cargo run --manifest-path crates/reel-workflow-ui/Cargo.toml --example synthetic --locked -- advance F:/YOUR-OWN-NEW-SAMPLE
cargo run --manifest-path crates/reel-workflow-ui/Cargo.toml --example synthetic --locked -- retain F:/YOUR-OWN-NEW-SAMPLE
```

`advance` introduces an unselected drawing candidate; `retain` records synthetic
retention. Neither changes the pinned source board or selected artwork. The sample
also writes `comic-board.json` for the alternate page label; pin it separately.

## Qualification and remaining acceptance

```text
cargo fmt --manifest-path crates/reel-workflow-ui/Cargo.toml -- --check
cargo clippy --manifest-path crates/reel-workflow-ui/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path crates/reel-workflow-ui/Cargo.toml --all-targets --locked
cargo test -p reel-assets --bin reel-editor-host --locked
node --check crates/reel-workflow-ui/src/board.js
```

Windows tests exercise the actual host, hostile routes, lazy bytes, range/HEAD,
tampering refusal and live candidate observations without selection changes.
Portable film tests inspect actual synthetic Git commits and exercise the CLI:
exact Spanish text despite dirty replacements, independent bilingual clocks,
stable C IDs across drawings, preserved historical/multiple approvals, missing
history, wrong logical binding, borrowed take previews, pins and no-overwrite.
The existing editor-host regression tests cover the extracted shared transport.
Desktop/narrow visual and browser playback acceptance remain required: the
available browser host was unavailable during author qualification. This slice
is not ready for production adoption based only on these protocol tests.

Native preview-provider integration, complete multi-scene project projection, additional
music/effects/review-film shelves and W4 issue/fix/rebuild actions remain separate
work. Generic git-asset registration/guarded preview requirements are tracked in
[git-asset #75](https://github.com/giodl73-repo/git-asset/issues/75).
