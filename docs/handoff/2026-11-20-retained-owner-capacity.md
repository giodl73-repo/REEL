# Retained history capacity diagnosis

The specific stop below now has a qualified
[original-operation recovery](2026-11-20-retained-capacity-recovery.md). The control
is Retained and technically complete under its unchanged original runtime.
The observations below preserve the earlier pending-state diagnosis.

The [derived repair control](2026-11-20-derived-repair-edges.md) produced its
assembly request under the original 264efd runtime, then stopped during managed
retention. At that stop its original journal was ImportPending, with input/output claims,
worker files and response metadata preserved. No retained acknowledgement or
technical completion was then established. Do not retry the fixture or replace the
original operation, attempt, source profile, target or runtime.

The read-only Rust/CLI diagnostic passed in 1.37 seconds at source
`fc35968f3bce3dfe2d34b0c95ce45371e3ff0656`. It reopens the actual original
activation, validates complete response-registration metadata, holds the source
and retained-owner fences, and measures the exact same union/path layout as the
registration code. It performs no payload reads, retention staging or mutation.

| Observation | Actual | Existing limit |
| --- | ---: | ---: |
| Source actor objects after response registration | 75 | — |
| Retained owner objects before this retention | 473 | — |
| Combined unique objects, including roots/evidence | 525 | 512 |
| Combined logical bytes | 270,429,684 | 1,073,741,824 |
| New canonical paths | 52 | 128 |

The object-count limit fails; byte and changed-path limits pass. The initial
input admission was 65 objects / 189,660,512 logical bytes. That input closure is
not the cumulative retained-owner history. No original retention work is staged;
the prior response-registration work is complete. This diagnosis does not by
itself establish current payload availability or retained ownership.

The exact operation remains `rooted-issue-control-original`, attempt
`rooted-control-attempt-1`, task
`c116b7dddd59a15c8d28eee16831bf9ac8594f99fc4a19d5c7e39c7f4bc47589`,
activation `25e841600e8aa93861877f357eae98b84443b29769501862e646790bb5f119e6`,
source WorkRef
`work-63343abc3b3600ed91e97a312fa2d5b25076516372e74000b735f902c9370d25`.
Its source commit is `c184b3db9b94e6001b91c3b69c2546a82750b7c1`. The exact Index
checkpoint is `b716971f7cfaf874074b48bafa691e3d8dac410e736bab9cbcd7887d8a9be0cf`.
Journal, packet/profile/receipt/operation controls and Index remain unchanged
through both the Rust API inspection and actual CLI invocation.

The CLI is:

```text
reel-workflow-native inspect-retention-capacity PACKET SOURCE ADMISSION RECEIPT RESULT TARGET
```

It reports bounded numeric metadata and original retention-work status. It does
not authorize widening limits, repeat publication, finish the task, return the
slot, retire scratch or release any history. Changing a frozen snapshot budget
would change the task and cannot repair this operation.

The recovery boundary must distinguish an individual job's projection allowance
from the retained owner's cumulative-history policy. An original-operation
capacity recovery must be a concrete bounded plan: pin the original receipt,
activation, WorkRef, response/candidate identity, unchanged target owner/revision,
exact required union/delta and finite proposed owner allowance. Recheck those
conditions under the original fences. Preserve the 128 changed-path and 16 actor
bounds and every old reference. Existing or uncertain retention work must be
reconciled, not replaced. Reopen payloads and prove retained-owner acknowledgement
before the original journal can complete. A coordinator-only recovery must not
silently adopt a new worker runtime; any required protocol/profile adoption needs
its own explicit qualified transaction and rollback boundary.

There is no justification to reset the owner, delete earlier references, initialize
another cache, silently enroll an actor or issue another render to escape this
bound. The native control is now qualified through the linked bounded recovery;
the complete corrected issue still requires assembly and contextual review.

Evidence is in `F:/src/tig-dev-temp/reel-retained-capacity-20261120-v1`:

- 1,978 source files verified; source-manifest SHA256
  `d3f48e2bacbc069fa49cb4a0634de934992e029003db359d7522161f006ab932`.
- Diagnostic CLI SHA256
  `48101ff616c8e5f25540737b558874778953a7496d6e9fb49d5a28db457472cf`,
  25,096,704 bytes; fixture SHA256
  `95af0333790d5ed999d92233785879ba09873d60a4daff14d39ed04f3c1b53ef`,
  30,286,848 bytes. These are inspection tools, not adopted lifecycle runtimes.
- `qualified-report.json` SHA256
  `419e64b63ccd1e69075ddd68ff4cd249e7a6696305a8c7175fdf19082358ea56`
  pins 51 source/tool/failure/original-operation/inspection artifacts.
- `qualified-validation.json` SHA256
  `fbe0d8e8c4717440677f67356357aec3afdf74108866e9b3a8a70f3f432a1aa9`:
  143 workflow and 86 standard native tests, all 247 SDK source pins and strict
  workflow/native Clippy pass. The 84 opt-in native tests are excluded from that
  standard native run; the actual metadata diagnostic is qualified separately.

Producer-applied comics-editor and rights/provenance checklists preserve exact
history and separate technical readiness from viewing/creative approval. No
actual finding closure, selection, private/public delivery or release occurs.
