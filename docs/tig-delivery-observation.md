# Reconcile a retained delivery inventory

`reel-tig-delivery-observation PROFILE NEW_PRIVATE_REPORT` compares a pinned
JSONL file-metadata observation with exact committed Tig targets. It does not
enumerate a mounted folder or contact a provider. Build with `tig-history`.

```text
exact Git commit + record/definition scope
                 |
authored schema + admitted retained-receipt policies
                 |
verified named receipt metadata -> exact target identity + published subject
                 |
pinned inventory -> explicit namespace mapping -> per-file reconciliation
```

The private profile uses schema `reel.tig-delivery-observation.experimental-1`:

- `source`: the existing `reel.tig-input-profile.experimental-1`, with an exact
  commit, explicit scopes, existing metadata byte provider and empty `admitted`.
  Definition admission for rendering is neither needed nor granted.
- `inventory`: `{path, sha256, bytes}` for an existing bounded JSONL observation.
  Each line has `relative_path` and integer `bytes`; other observed fields remain
  uninterpreted. Duplicate paths, traversal and pin mismatches refuse.
- `policies`: a named map of exact local `{path, sha256, bytes}` receipt policies
  accepted by the project. These use Tig's existing publication-evidence schema.
- `mappings`: explicit `provider`, `scope`, `definition_id`, policy names in
  `policies`, `observation_prefix` and `receipt_prefix`. Literal prefix substitution
  maps observed paths to the retained receipt namespace. Optional `opaque_locator`
  selects Tig's opaque-locator identity; `backslash_separator` explicitly converts
  suffix separators for old Windows locator strings. Defaults are false.
- A mapping may instead provide `correspondence`: a pinned external `table`
  (`path`, `sha256`, `bytes`), `rows_pointer` to an array,
  `observed_path_pointer`, `receipt_target_pointer`, `observed_path_prefix`, and
  optional `backslash_separator`. Each row's observed path must start with the
  exact declared prefix; removing it and optionally converting backslashes gives
  the complete inventory-relative path. Its target is interpreted using the
  mapping's existing provider/scope and `opaque_locator` setting. Provider IDs
  therefore remain stable even when the observed display filename changes.
  The ordinary prefix fields must be empty and their separator flag false in
  this mode. No fallback to prefix substitution or basename matching occurs.
  Duplicate paths or target identities, invalid pointers, traversal, missing
  fields and pin mismatches refuse before writing a report. Tables are bounded
  to 4 MiB each, 16 MiB and 65,536 rows collectively, with depth/node limits.
  The profile's existing 64-KiB and 128-mapping bounds remain unchanged.
- `client_metadata_basenames`: exact exclusions such as `desktop.ini`. Authored
  README, navigation and support files have no implicit exclusion.
- `max_report_bytes`: explicit bounded allowance, at most 64 MiB.
- Optional `succession`: `{ "pointer": "/metadata/predecessor",
  "relationship": "same delivered target; explicit successor" }`. This is an
  explicit project interpretation of an existing committed declaration, not an
  implicit status or timestamp rule. At the pointer, an object must contain that
  exact `relationship`, a predecessor `id` and its exact `subject` (`sha256`,
  `bytes`). Both anchors must pass their own retained-receipt checks, and the
  predecessor's subject must match. Missing bindings, forged pins, self-links
  and cycles refuse. With no rule, predecessor metadata has no effect.

Only records with an authored `target` containing `provider` and `scope` are
examined. Unmapped definitions/scopes and unverifiable receipt bindings remain
explicit binding gaps. A project embedding targets elsewhere needs an explicit
adapter; this command does not claim it found every possible target convention.
Missing admitted policy is not evidence that the asset lacks a delivery anchor.
Retained opaque provider IDs cannot be matched to a mounted path without an
explicit verified identity mapping; file lengths do not supply that mapping.
Correspondence only supplies that declared association. It does not replace the
independent subject/target receipt check or prove that the observed bytes are
current. Original external metadata and its hash stay unchanged.

Every observed row is classified as client metadata, unmatched, ambiguous,
different length, or retained target with matching length. A same-length match
does **not** establish current content, provider identity, sync, custody, approval
or eviction eligibility. Unobserved binding IDs are reported separately. Several
logical anchors claiming the same target remain ambiguous even for identical
bytes unless an explicitly admitted succession resolves them. A predecessor is
historical only on a row where both checked target identities map to that exact
observed path, including explicit cross-scope mappings. The report preserves all
`candidate_ids` and separately reports `current_candidate_ids` and
`historical_candidate_ids`. Forked successors remain ambiguous; a successor's
length mismatch remains visible even if the older observation matches its
predecessor. No timestamp, basename, case folding or hash-only deduplication chooses
between them. Current selections, prior versions and publication state are never
changed. Only named receipt metadata is opened; media payloads are not read.

Stdout is a count-only summary. The no-clobber private report includes file paths,
candidate IDs, frozen source and input pins, successful receipt checks and gaps.
Keep it in asset storage through the existing publisher, not Git. A candidate
build or an observed matching length does not qualify a production lane.

Producer-applied rights/provenance and platform/audience review: private reports
stay distinct from portable receipts; immutable publication subjects and human
approval remain unchanged. Fixtures cover an actual committed source with a
dirty checkout, verified receipts with no payload objects, corruption, pin
tampering, duplicate observations, no-clobber reports, exact path matching,
length differences, competing anchors and explicit client metadata.
The external correspondence fixture uses 147 rows and a table larger than
64 KiB, with no payload objects. It also checks stale pins, duplicate JSON keys,
duplicate paths/targets, missing targets, invalid pointers, prefix escapes,
traversal and conflicting modes before output creation.
Succession fixtures additionally cover cross-scope historical anchors, stale
inventory lengths, competing successors, different paths, missing/forged
predecessors, self-links, cycles and explicit relationship admission. These
metadata classifications grant no current remote-content or eviction claim.
