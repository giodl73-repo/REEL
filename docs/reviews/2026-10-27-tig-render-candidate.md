# Native rendered-candidate handoff contract

Entry REEL 05a0a03b4ea8a46e6e8abc52008367a6aa1fdcd1 (PR190, qualification
pending). Continues WP-010/011/012, REQ-006/009/010, SPEC-010/011/012,
IF-005/010/011, PKG-004/006. Optional Rust producer API only; no CLI/runtime
adoption, provider initialization, transfer, selection, eviction or historical
render. Tests execute synthetic media only.

The caller supplies an already verified frozen ProductionContext, admitted
Source/request and explicit producer package Object. The caller owns durable
finish/reopen/readback, owner/job enrollment, resource admission and input custody
before execution. Match recipe checkpoint, binding, admission and parameter pins
against that context before output creation. Compile the job inside this call
from the admitted native source, prepare its exact guarded inputs, render and
independently decode/check. Do not accept an unrelated caller-authored render job
as evidence for a scene recipe.

Return a sealed in-process rendered candidate containing original recipe/context,
the exact private receipt pin and checked output identities. To issue an unsigned
candidate claim, require explicit output-name to separately bound asset-subject
mapping, exact receipt readback and fresh output hash/size verification. Neither
a bare claimed Object nor a forged deserialized receipt constructs the sealed
candidate. Claim creation alone persists nothing, enrolls no demand and grants
no approval, current selection, delivery currency, retained custody or deletion.

The externally admitted producer Object is still not verified complete executable/
FFmpeg/library/font closure. The operation remains the conservative declared-input
recipe. This slice proves local execution and checked candidate bytes, not full
renderer-freshness acceptance. No new global cache/render lock; existing read
guards remain scoped to execution and the caller still owns output custody.

L0: fmt, default-library isolation, warnings-denied all-feature Clippy, vendor and
diff integrity. L1: wrong context/request/source refuses before payload/output IO;
wrong mapping, unknown output, duplicate/empty output claims, changed receipt and
tampered payload refuse. L2: temporary Git, explicitly finished/reopened metadata,
restored native source, real tiny Windows FFmpeg render/independent check and new
candidate claim round trip; no actual user media. Qualified parent and exact-head
Windows/Linux checks precede merge.

Producer review lenses: Rights/Provenance protects exact private pins and separates
execution assertions from authority; Editor protects native job/scene binding and
independent older jobs. These are producer checklists, not independent review or
human acceptance. Runtime closure, all live job/resource claims, source/variant
completeness, recovery, output publication/custody and reviewed adoption remain open.

## Local execution and producer review

Windows: all 25 normal Tig consumer tests pass. A foreign language context or a
new source checkpoint with an otherwise identical recipe refuses before payload
reads and job/output creation. The new explicit tiny FFmpeg case passes after
SQLite finish/reopen/readback, native context restore and isolated owner-journal
prepare/adopt. It compiles inside the producer API, renders, independently checks
and creates a matching claim that passes exact original-plan validation.

The same case rejects empty, duplicate, oversized and unknown-output mappings,
foreign subject pins, same-size corrupted bytes, truncated bytes and changed
receipts. Restoring original bytes makes the candidate claimable again; overwriting
existing job/output directories refuses. Input readers are released after the
operation and the fixture journal still declares incomplete global coverage.
The Windows workflow now runs this explicit case alongside the existing native
CLI render/archive regression. Default-library isolation, fmt, vendor integrity,
diff and all-target/all-feature warnings-denied Clippy all pass locally;
exact-head cloud qualification remains required before merge.

Rights/Provenance finding: never deserialize a success envelope into a completed
producer capability. The candidate constructor remains private; creation uses
actual native compile/render/check, and claim issuance rechecks receipt/output
bytes. Full producer package/tool closure and output custody remain open, owned
by integration; no freshness/retirement acceptance is inferred. Editor finding:
an arbitrary job path cannot be paired with a native context; compilation now
occurs inside the handoff. Producer disposition pass_with_risk for checked local
candidates only, not independent or human acceptance.
