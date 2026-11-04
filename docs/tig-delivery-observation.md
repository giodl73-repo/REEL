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
- `client_metadata_basenames`: exact exclusions such as `desktop.ini`. Authored
  README, navigation and support files have no implicit exclusion.
- `max_report_bytes`: explicit bounded allowance, at most 64 MiB.

Only records with an authored `target` containing `provider` and `scope` are
examined. Unmapped definitions/scopes and unverifiable receipt bindings remain
explicit binding gaps. A project embedding targets elsewhere needs an explicit
adapter; this command does not claim it found every possible target convention.
Missing admitted policy is not evidence that the asset lacks a delivery anchor.
Retained opaque provider IDs cannot be matched to a mounted path without an
explicit verified identity mapping; file lengths do not supply that mapping.

Every observed row is classified as client metadata, unmatched, ambiguous,
different length, or retained target with matching length. A same-length match
does **not** establish current content, provider identity, sync, custody, approval
or eviction eligibility. Unobserved binding IDs are reported separately. Several
logical anchors claiming the same target remain ambiguous even for identical
bytes. No timestamp, basename, case folding or hash-only deduplication chooses
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
