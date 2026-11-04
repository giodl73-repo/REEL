# Register retained files from a delivery table

`reel-tig-register-deliveries PROFILE NEW_DIRECTORY` creates initial independent
file-anchor candidates from one exact retained table. Build with `tig-history`.
It uses the supplied project's definition/schema and Tig's existing publication
checker, not a project-specific converter. It reads the receipt metadata only.

The private profile schema is
`reel.tig-delivery-table-registration.experimental-1`:

- `byte_source`: explicit existing receipt metadata reader; no store is created.
- `receipt`: exact SHA-256 and length of the original table.
- `rows_pointer`: JSON Pointer to its table; `row_layout` is `array` (default)
  or `object`. `asset_pointer` and `path_pointer` select each row's immutable
  subject and target. Object member names are escaped as JSON Pointer tokens,
  preserving `/` and `~` without guessing a selector.
- `target_kind`: `logical-path` (default) validates a project-relative path;
  explicit `opaque-locator` preserves a bounded literal legacy locator such as
  a Windows destination or provider ID. Locators are never opened as paths.
  The admitted receipt policy must assert the same target kind and exact identity.
- `target_from_member_key`: for object tables only, use each exact member name
  as its target. With `asset_pointer: ""`, this supports path-keyed subject objects
  containing exact `sha256` and `bytes`. Bare hash strings cannot supply a length
  and are not inferred into complete subjects.
- `provider`, `scope`, `id_prefix`: explicit target namespace and stable logical
  ID prefix. Each ID and physical filename derives from the scoped target hash,
  independent of payload bytes and row order. Equal bytes never collapse targets.
- `definition`: the existing exact `DefinitionFiles` local pins; `receipt_policy`
  is a local FileRef to the admitted publication-evidence policy.
- `authored`: project fields required by that definition, excluding `target`.
- `provenance`: bounded named original source/package pins. These remain exact
  evidence, not inherited approval or newly asserted rights.
- `max_rows` and `max_output_bytes`: bounded allowances, at most 4,096 rows and
  16 MiB of candidate records.

Each candidate has a selected retained subject, a `retained-readback` stream,
its original `delivery-receipt`, and provenance pins. Every authored schema and
receipt binding is checked before any directory is written. The policy's exact
target identity must equal the explicitly mapped row path; unrelated IDs or a
convenient alternate field cannot silently substitute for that path. Duplicate
targets, malformed paths, mismatched cache URIs, unsafe pointers and budget
overruns refuse. Existing output directories are never overwritten.

Large external ledgers use a separate 4-MiB metadata intake bound and strict
duplicate-key rejection. The native Tig record limit remains 64 KiB. A large
receipt is not permission to put an oversized inventory in Git. One immutable
parsed receipt is reused while Tig rechecks each candidate's pins and limits.

This is an **initial import**, not a daily version updater. Review the proposed
namespace against the existing project before adopting any new targets. Do not
replace existing records wholesale from a rerun: that could discard their prior
streams. Use Tig's existing version/history update contract for subsequent edits,
keeping unchanged asset records and earlier publication evidence intact. Stable
candidate IDs do not by themselves establish preserved version history.

No media availability, current remote publication, custody, musical acceptance,
production claim, approval or eviction authority is inferred. The adapter does
not render, copy media, upload, change Git, adopt candidates or bind producers.
Keep raw profiles/tables/evidence asset-side. The small candidate `.tig` files
can be reviewed, checked and committed through the normal project workflow.

Producer-applied rights/provenance and platform/audience review keeps exact
retained subjects separate from release authority. The actual fixture has no
payload objects and exercises a ledger beyond 64 KiB, identical bytes at two
targets, stable IDs through reordering and a byte revision, unchanged earlier
candidates, duplicate targets, traversal, cache-URI mismatch, target-policy
disagreement and no-clobber output. A real project still needs complete file
classification and byte availability checks; registration alone is insufficient.
