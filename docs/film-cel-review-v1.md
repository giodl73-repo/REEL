# Stable cel IDs in private review movies

`reel-film-review` makes an explicitly requested H.264 review derivative from an
existing native scene-delivery `review.mp4`. It burns a visible C/story ID strip
and `REVIEW` badge into the video and writes an exact frame-to-target map for
issue/fix consumers. It copies the source's compressed audio packets and checks
every video presentation timestamp and audio packet/timestamp against the source.

```text
cargo build --bin reel-film-review --locked
reel-film-review capabilities
reel-film-review build REQUEST.json RAW_SHA256 --root INPUT_ROOT --output-dir NEW_DIRECTORY
reel-film-review check REQUEST.json RAW_SHA256 --root INPUT_ROOT --output-dir EXISTING_DIRECTORY
```

`build` creates a new directory exclusively, with an existing owned parent.
It never overwrites a previous review. Failure preserves the new scratch and
does not record a completion receipt. `check` reopens and checks an existing
review; it does not repeat the full render. No model, speech service, cache
initialization, hydration, upload, selection or native retirement runs.

## Source and identity binding

The strict `reel.film-cel-review.v1` request contains:

| Field | Meaning |
| --- | --- |
| `project`, `source_commit`, `language` | Declared provenance and language for this review. Commit is a full nonzero hex ID; this producer does not resolve Git history or authenticate that declaration. Exact input pins remain mandatory. |
| `film` | Source `FileRef` (`path`, `sha256`, `bytes`) for the existing delivery's `review.mp4` |
| `delivery_receipt` | Exact native `reel.scene-delivery-receipt.v0.1` |
| `job` | Exact original `reel.scene-delivery.v0.1` job; its pin, contract and production identity must agree with that receipt |
| `font` | Explicit pinned TrueType face, up to 32 MiB; required ID glyphs and measured text width must fit |
| `font_size` | 10–64 pixels; the ID strip covers the top `font_size + 16` pixels of this private derivative |
| `identities` | Every actual picture attachment ID → `{story_id, cel_id, semantic_id}` from the producing project's stable identity mapping |
| `limits` | `{timeout_seconds, input_bytes, output_bytes, probe_bytes, frames}`; explicit bounded admission described below |

The source film must be the exact `review.mp4` output in the receipt's original
relative namespace, not a different file with coincidentally matching bytes.
Every delivered picture must have exactly one identity. IDs do not derive from
drawing filenames/hashes. The map preserves each picture's exact source `FileRef`.

Labels use the delivery receipt's **actual picture frame intervals**, including
explicit delivery-frame allocation. They do not round sample positions again or
borrow another language's clock. Gaps, overlaps, duplicate occurrences, missing
targets, incomplete coverage, different source geometry or a noncontinuous source
frame clock refuse. The existing native scene producer remains responsible for
the original picture/audio/conform proof; this is an additive review derivative.

The output `review-map.json` records the request pin, source film, job, delivery
receipt, dimensions, rational frame rate and exact half-open frame ranges. A
finding can bind a C ID to its attachment, semantic event, selected drawing and
original review baseline instead of guessing from a timestamp or latest drawing.
Each scene's review can be consumed by a separately qualified episode assembly;
this command does not infer complete-film offsets or claim episode qualification.

## Provider and lifecycle boundary

`film_presentation_overlay::review::{build_with_inputs, check_with_inputs}` reuse
the existing `scene_delivery::InputFiles` provider interface. A protected native
caller supplies verified paths, pinned media programs and live source guards for
the entire synchronous operation. Explicit provider errors never fall back to
ambient tools. Local CLI compatibility uses ordinary pinned relative files and
installed FFmpeg/FFprobe; it establishes no native record-to-annex custody.

Output is an ordinary candidate package: movie, frame map, label files, filter
recipe, exact font and technical receipt. The existing protected handoff/import
interfaces must separately publish, reopen and acknowledge retained ownership.
This command neither completes a workflow task nor releases its claims/capacity.
The receipt explicitly leaves `native_retention_verified`, `approvals_verified`
and `visual_reviewed` false, with publication unauthorized.

Only the selected video and audio streams of the scene review are carried. This
does not create captions, credits, portable language editions or new performances.
Existing burned presentation remains in the source picture; the top review strip
is intentionally a temporary viewing annotation, not a creative master.

## Bounds and qualification

One operation admits at most 2,048 picture occurrences, 1,000,000 frames, 64 GiB
declared source bytes and 16 GiB output, within an explicit 1–18,000 second total
media-process deadline. Controls and retained metadata are bounded; each probe
response has an explicit 1 KiB–512 MiB cap. FFmpeg gets an output-size ceiling,
and its result must still satisfy complete frame/audio closure before completion.
Processes use the existing deadline/reaping helper and bounded diagnostic drain;
media inputs permit only local file/pipe protocols. No full movie is loaded into
memory. The frame/packet proof is bounded in-memory metadata, not a new cache.

The filter recipe is loaded from a file using FFmpeg's documented
[`-/filter:v` argument-file syntax](https://ffmpeg.org/ffmpeg.html#Options), keeping
episode-sized label lists out of Windows command-line length limits. An installed
build lacking that syntax or `drawtext` refuses; no fallback changes the recipe.

Synthetic media tests render a real scene receipt, use a sample boundary between
frames, check visible label changes at the actual delivered cut and recheck
unchanged timestamps/audio. Refusal cases cover wrong source identity/namespace,
missing targets, budgets, explicit program denial, wrong request pins and altered
label evidence. Tests do not establish full Season 5 scale or creative approval.

```text
cargo test --test film_cel_review --locked
cargo test --test film_presentation_overlay --locked
```

W4 still needs controlled findings, issue drafts, expected-baseline fixes,
affected-language/consumer rebuilds and protected workflow dispatch around this
producer. Production adoption, installed tool pins, historical recovery and
episode-scale composition remain their existing gates.
