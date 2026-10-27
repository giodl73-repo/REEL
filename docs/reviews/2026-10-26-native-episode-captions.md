# Native episode captions: producer-applied review

Review was producer-applied against `.roles/ROLE.md`, Editor, Platform and
Audience, and Rights and Provenance Steward. These are checklist findings,
not independent human approval.

- Editing: exact native ordered sample offsets, whole source duration, repeat-only
  frame counts and explicit silent holds are verified; no source cue is clipped.
- Platform/audience: separate SRT and VTT preserve exact selected source payloads,
  line breaks and explicit inherited host placement. Visual/device readability
  remains an upstream caption-family review; no image was displayed here.
- Rights/provenance: selected master/source receipt and caption hashes remain
  separate from opaque owner selection/rights evidence. No new text, translation,
  voice identity or approval is generated. Native decoding and human selection
  stay with their existing gates; receipt remains explicitly private.

Synthetic tests passed for bilingual 27-unit clocks (13 intros, 13 films, outro),
all 27 two-second tail holds including terminal outro, and 24→25 / 25→30 clocks.
Eighteen invalid metadata/hash/order/path/hold cases fail before output creation.

Private retained comparisons passed for all eight prior programmes, using exact
SHA/byte-verified named E-cache controls read only and OS-temporary fixture
staging. Source JSON timing was converted to SRT inside the ignored Rust test;
no media was rendered, changed or uploaded. Every cue payload and order matched;
maximum timestamp difference was <=1ms owing to documented inward rounding.
Host VTT `line:70%` placement counts matched the retained VTTs.

| Programme | Exact cue payloads |
| --- | ---: |
| J1 ES | 191 |
| J1 EN | 189 |
| J2 ES | 207 |
| J2 EN | 200 |
| J3 ES | 169 |
| J3 EN | 163 |
| J4 ES | 230 |
| J4 EN | 225 |

The source package controls were pinned by
`11e54c6260a6fc89621dc4907d03ca05b657bef87c166336ab5846f84abc2378`
(26514 bytes) and
`2e8c02e8554d6b63d736d2689b28b41c63d65384415ba7713566e4d43128be65`
(8914 bytes). Exact evidence TAR, frozen-job/request, cue-caption TAR and expected
SRT/VTT refs were followed by role, without a cache census or jobs.

The expanded programmes still require their actual new host captions and final
native conform receipts. This generic capability does not claim the eight new
deliverables have been assembled or approved.

New expanded-programme CVA intro/full-film conform integration remains pending in
the producing lane; no new native receipt existed at port handoff.

Validation: caption integration tests 5 passed (private test explicitly ignored by
default, then run and passed for all eight staged fixtures); 14 native conform
unit tests passed; all-target/all-feature Clippy with warnings denied passed.
Changed Rust files pass rustfmt. Whole-repository cargo fmt check reports only
pre-existing formatting in browser_composition.rs and scene_delivery_compile.rs;
those files were left unchanged. Git diff whitespace check passed.

Managed-runner follow-up: precreated empty output directories are accepted,
existing nonempty directories fail without writes, and each retained file uses
exclusive creation. Tests cover the runner-compatible empty directory, preservation
of unrelated existing content, and explicit selected empty SRT for wordless units.
All eight private comparisons and all-target/all-feature Clippy passed again.
