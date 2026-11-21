# Retained spoken editions through the existing native SDK

**Status:** M05 merged as `027164f` in [PR #383](https://github.com/giodl73-repo/REEL/pull/383)
after required hosted checks passed, based on merged M04 (`c59b490`, PR #382).
Local and hosted qualification are recorded in the [M05 review](reviews/2026-11-21-m05-spoken-history-review.md).
No new cache, custody
engine, automatic restore or project cutover is introduced.

## Existing interfaces and ownership

| Requirement | Reused owner |
| --- | --- |
| Exact committed native definitions, records and admitted payloads | `tig_inputs::Source`, pinned git-asset SDK snapshot/resolver |
| Protected synchronous inputs | `Source::prepare_files`, `tig_inputs::Inputs` |
| Frozen original source/request/runtime context, without the checkout | `Source::freeze_context_for_ids`, `ProductionContext`, `ArchiveProfile` and replay |
| Consumed identity distinct from full source provenance | `Source::projected_recipe_for_ids`, SDK `Consumption` / `PayloadConsumption` / `Operation` |
| Exact decoded output and text evidence | M04 checker; no receipt-only admission |
| Native output subjects and unsigned correspondence | SDK `BoundSubject`, produced `Claim` |
| Bounded publication into explicitly enrolled existing stores | Existing `Publication`, `DeliveryBudget`, `publish_checked`, `ExistingPublisher` |
| Persistent metadata finish/readback | Caller-owned SDK `NodeStore`; existing `MetadataStore` / `MetadataReader` |

All generic SDK bytes retain source pin `0ceade2a16de1c1ed6cc27e48675e7945d7b61df`.
Missing generic behavior belongs upstream. REEL owns the spoken interpretation,
native adapter and independently checked output bindings. Producing projects own
asset definitions/versions, source decisions, selection and deployment.

## Native operation contract

The caller explicitly supplies admitted native IDs for the declaration, exact
producer executable, optional processing policy, and participating definitions.
The declaration's selected FileRefs use native asset IDs. There is no filesystem
fallback, latest lookup or discovery of cache roots.

Preparation holds the complete selected source/alignment/runtime guards through
compilation and checking. Script-authority and reuse-evidence pointers are also
retained as exact opaque inputs; byte verification does not approve their
contents. Runtime bytes are independently read and matched to the admitted
producer pin. Production additionally requires the running executable to match;
historical checking may bind an earlier retained executable without running it.

Freeze one context per edition in the SDK's existing production-context format.
The original declaration and policy remain captured controls in every context,
with their own definitions distinct from consumed payload types. The private
native receipt retains the exact full request. Compute one consumed recipe per edition. Its operation
parameters contain that edition's canonical consumed recipe and explicit
processing, not the aggregate registry or a changing global selection log.
Each context descriptor is exactly that edition operation's parameters, as
required by SDK produced-claim replay. Only its selected wave/alignment/script/
scope/runtime inputs enter its projected native payload fingerprint. Index and
global policy payloads are explicitly excluded from that fingerprint because the
canonical parameters contain their consumed per-edition meaning. They remain
retained, protected source controls. The aggregate declaration remains source
provenance and is not represented as complete consumed semantic coverage.

Replacing Spanish technical detail v1 with v2 must change only Spanish technical
consumption. Context/checkpoint/aggregate provenance can change while English
technical and both executive consumed recipes stay exact. A native recipe alone
does not prove the producer used those inputs: M04's reopened PCM check remains
mandatory before publication.

## Retention and reopening

A checked retained candidate binds:

- original frozen context, source checkpoint and metadata archive;
- exact declaration, plan, processing and producer bytes;
- selected source/alignment/authority/reuse references;
- candidate WAVs, private producer receipt, access manifest and transcripts;
- per-edition compiled/execution/native consumed recipe identities; and
- native output subjects, separate review bindings and actual publication ACKs.

Use the existing publisher for explicitly requested retained payloads; preflight
all capabilities, exact pins, object counts and aggregate bytes before its first
write. Reuse the existing payload → receipt → claim ordering and exact retry
keys. Failures can leave acknowledged prefixes; they do not constitute an atomic
batch, permanent custody, selection, review approval or release.

The caller must finish/read back metadata nodes and adopt the returned roots
separately. Reopening requires the original archive/context selection, original
producer and exact retained controls/payloads. It must work with Git and working
metadata unavailable. Missing payload/runtime or stale review reports explicit
failure; it must not hydrate, regenerate, substitute, evict or delete anything.

Review bindings cover exact edition recipe, runtime, wave and text package.
Changing a take/join/mix/text stales affected bindings. Unchanged consumed recipe
identity alone cannot preserve approval when the actual bound output differs.
No human review or authority is granted by creating or matching a binding.

## Bounds and verification

The engineering reference remains the four-edition EN/ES signal fixture. Native
context descriptors are capped at 64 KiB, source references at 512, graph nodes
at 1,024, graph edges at 1,024 and traversal depth at 4. Native runtime objects
are capped at 128 MiB; total retained input/output publication is capped at 256
MiB and 1,024 distinct objects. These caps include the fixture's actual executable
and all requested retention objects. Never raise limits implicitly after a
failure. M03/M04 PCM, duration, clipping and processing limits remain unchanged.

Metadata uses the SDK's bounded NodeStore/tree limits. Caller-defined tighter
limits still apply; provider durability stronger than its capabilities refuses.
Do not describe a metadata archive as retained payload bytes. Explicit retained
publication and exact reopening must prove availability separately.

Windows qualifies `PreventMutation` protected input reads. Linux must reject that
path explicitly; pure compiler/checker tests remain portable. A cooperative
reader is not substituted to manufacture native protection parity.

Required `spoken_programme_history` tests under `--features tig-history`:

| Case | Required evidence |
| --- | --- |
| `spanish_detail_change_preserves_other_consumed_recipes` | Four compiled/execution/native IDs compared; only Spanish technical changes; original immutable payloads stay retained |
| `retained_v1_reopens_and_stale_reviews_refuse` | Finish/readback metadata, reopen original context with checkout unavailable, independently check original WAV/text and reject an altered review target; the selective-change test proves actual take replacement stales only affected review bindings |
| `unsupported_protected_input_refuses` | Exact unsupported refusal on platforms without mandatory path protection; no fallback or writes |
| Missing/adversarial inputs | Missing retained payload/runtime, edited context/controls, changed output/receipt, insufficient publication budget/capability; no hydration or success/promotion |
| Publication/retry | Existing backend ACKs exact selected outputs and retained controls, failure/retry uses stable operation keys; no overwrite or deletion of retained versions |

Record exact commands, fixture/runtime pins, platform and result. Run relevant
M04/core tests, all-feature lint, formatting, `git diff --check` and hosted checks.
Editor, Rights and Sound are required role lenses; use Music Reconstruction for
consumed/signal identity and Platform/Audience for privacy and delivery evidence.
Actual listening, selection, voice/text authority, adoption and release remain
unperformed unless their own exact governed decisions are supplied.
