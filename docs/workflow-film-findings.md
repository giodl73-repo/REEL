# Exact-baseline film findings and issue drafts

`reel-workflow draft-film-issues REQUEST SHA256 NEW_DIRECTORY` checks reported
findings against an exact cel-ID review movie, frame map and technical receipt.
It exports local private drafts and GitHub API body files. It does not contact
GitHub, apply a repair, update selection or authenticate viewing.

## Runnable synthetic walkthrough

With installed FFmpeg/FFprobe and the standard Windows Arial or Linux DejaVu Sans
font available, choose a new absolute directory with an existing owned parent:

```text
cargo run --example film-review-workflow --locked -- NEW_ABSOLUTE_DIRECTORY --apply
```

This Rust example generates two color cels and silence, renders the existing
native scene, creates the visible-ID review, checks it, and exports one clearly
synthetic finding. Open `review/review.mp4` and the Markdown under `issues` to
inspect the result. Controls, sources, frame map and receipts stay in that new
directory. No real project assets, accounts, model calls, native store, issue
POST, profile adoption or production selection are involved. Failures preserve
the directory. The example shares its fixture with the real-media regression.

This is a developer walkthrough of these components, not Season 5 acceptance or
an installed/retained production workflow.

The v1 map/receipt contracts live in `reel-assembly::film_review`, shared by the
[review-copy producer](film-cel-review-v1.md) and portable workflow consumer. The
producer re-exports those metadata types; JSON fields are unchanged. Its native
`InputFiles` access boundary remains separate. `reel-workflow` acquires no media
renderer dependency; the root's test-only workflow dependency exercises actual
producer bytes through the drafting path.

## Request and findings

The pinned export request has schema `reel.workflow-film-findings-export.v1` and
contains `report` plus `files`. `files` maps each declared logical artifact ID to
its explicit existing private file path. Undeclared, missing or conflicting
bindings refuse. The raw request SHA includes local bindings; keep the request
in project-owned private storage.

The report has schema `reel.workflow-film-findings.v1`:

| Field | Meaning |
| --- | --- |
| `review` | Exact `video/mp4` artifact for the copy with visible cel IDs |
| `map` | Exact JSON artifact for its `review-map.json` |
| `receipt` | Exact JSON artifact for the technical review-copy receipt |
| `findings` | Up to 32 observations on this same movie/language baseline |

Each finding supplies `id`, `target_id`, `frame`, `category`, `method`,
`observation`, and optional `evidence` artifacts. IDs are caller-owned stable
tokens. `target_id` is the exact picture attachment ID in the map. `frame` is
zero-based and must lie within that target's half-open delivered interval. A
target ending at frame 24 does not accept a finding on frame 24.

Categories: `picture`, `timing`, `audio`, `captions`, `continuity`, `source-text`,
`rights`. Methods: `human-report`, `model-report`, `deterministic-check`. These
describe the submitted report, not authenticated viewing or observation accuracy.
Observation text is preserved in JSON and quoted for Markdown display. No source
text, translation, drawing, performance, approval or clock is rewritten.

The movie/map descriptors must agree with the receipt. Its request pin, frame
count, clock proof and output-name closure must agree with the map. The original
technical receipt's unverified authority flags stay false; human decisions belong
in separate immutable evidence. Bad declarations and wrong targets refuse before
reading a large movie. The original source-commit field remains declared
provenance; drafting does not resolve Git history or verify native custody.

The local reader streams the movie through SHA-256 once per batch and verifies
each distinct evidence artifact once, retaining file handles through export.
Native callers may supply `film_review::Reader` over the existing qualified asset
reader, preserving its exact record binding and live guards. Ordinary local file
byte checks establish no native record-to-annex custody or retained ownership.

## Output, replay and later issue submission

Each finding produces `KEY.json` (structured draft), `KEY.md` (private readable
body), and `KEY-github.json` (`{title, body}`). `index.json` records the request pin
and ordered keys. At the 32-finding limit the package has 97 files. A native job
still needs its complete actual admission preview, including evidence and retained
publication transitions; output count alone does not qualify lifecycle bounds.

Keys bind movie, map, receipt, target and exact individual finding. Reordering a
batch or exporting a subset preserves each unchanged finding's key. Changing the
baseline, target, note or evidence changes its key. Old drafts keep their original
selected-picture subject. A newer drawing is never substituted.

The key fingerprints semantic input/version data, not a serialized file's bytes.
Published output artifacts still need their actual byte hashes and pinned runtime
provenance through the managed workflow.

`finding_identity` separately binds that baseline, target and caller-owned finding
ID. Refining the note/evidence changes the immutable draft version while keeping
this logical identity. An issuer can detect an existing logical finding and
require an explicit update rather than treating every refinement as a new issue.

An exact existing package is a verified replay. Modified, partial or extra files
refuse and remain untouched. Creation is exclusive, with failed writes preserved
for explicit recovery. `viewing_verified`, `authority_verified`,
`native_retention_verified`, `issue_submitted` and `selection_applied` remain false.

The Markdown records the draft version and a `reel-film-finding` marker containing
that logical identity. These assist later reconciliation; they are not
GitHub service idempotency. An authorized issuer must preserve the original
operation and reconcile an uncertain result before another POST. The existing
bounded HTTP/API path can carry the generated body under an explicit pinned
service request. Drafting creates no cache engine, job registry, durable claim,
provider retry policy or API effect.

These are private files. The body omits machine bindings and producer filenames,
but observations and asset identities may themselves contain private context.
Export grants no public release or permission to upload them.

## Limits, qualification and remaining work

The report is a 1 MiB control; maps/receipts allow 4 MiB each. The movie bound is
16 GiB and does not inherit a comic-only 64 MiB limit. Notes allow 4 KiB and each
finding allows 16 evidence references, with 64 distinct evidence artifacts per
batch, each up to 32 MiB. These component limits do not qualify real episode scale.

```text
cargo test -p reel-workflow --test film_review --locked
cargo test --test film_cel_review --locked
```

Tests cover stale/wrong targets, duplicate IDs, false authority claims, changed
bytes, Windows producer paths inspected on other hosts, batch/subset keys, CLI
pins/replay and preservation of partial packages. The real-media integration
renders an actual scene review and resolves its finding through the shared map
to the exact selected picture.

W4 still needs integrated issue submission/reconciliation, exact native
record/selection baselines for repair, affected rebuilds and protected dispatch.
This draft cannot authorize repair from a matching hash alone. No project
adoption or actual Season 5 issue filing occurs here.
