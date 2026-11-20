# Closing a legacy job using its retained recovery policy

The corrected issue control completed through
[explicit retained-capacity recovery](2026-11-20-retained-capacity-recovery.md)
under its unchanged 264efd runtime and v1 handoff binding. Its finite owner policy
is in the retained recovery record, not a retroactively changed ticket. Returning
that source slot requires a close operation which explicitly consumes the same
retained policy. The automatic binding-v2 path remains separate.

```text
reel-workflow-native prepare-close-with-policy JOURNAL PROFILE TARGET PLAN SHA256 NEW_TICKET --apply
```

The API is `closing::prepare_with_retained_policy`. It validates the caller's
canonical plan pin, original retained job/result/acknowledgement and unchanged
runtime, then exports a v2 close ticket. Its operation identity includes the exact
policy object as well as the original admission/result/acknowledgement, retained
target and closing coordinator pin. V1 tickets omit the new optional field and
retain their original serialization and operation domain.

The plan must be a `reel.workflow-retained-capacity-plan.v1` for this exact
packet/source/admission/receipt/result/target request. Its intrinsic bounds and
reservations must still validate. The adapter reopens its exact canonical object
through the original retained work selection and metadata store, and checks the
actual work-object/byte counts against the recorded allowance. A plan stored in
another task, a changed request/plan, matching bytes without the original work
binding, or an arbitrary larger policy cannot authorize this close. A job which
already binds its own policy cannot also supply this recovery override.

The existing close machinery verifies ready retained coverage and the original
activation, then passes the recorded cumulative-owner allowance to the unchanged
qualified closer. The source job's input/projection budget, task, attempt,
lifecycle executable, private profile and exact approvals stay unchanged. Source
clear and retained-layout transitions retain their separate 128 changed-path
limits and actor admission bounds. Current-layout canonicalization preserves all
objects and prior tree roots; immutable work selections retain their named links.
Staged or uncertain close work must recover its exact ticket and WorkRef, not a
new operation. Scratch retirement and history release remain separate operations.

The next opt-in fixture builds complete corrected-issue inputs before close: it
reopens actual retained opening/control and historical closing proofs, materializes
the v4 revision for issue assembly with exact producer/result witnesses and checks
that every already-produced parent packet is unchanged. The fixed scope is narrowed
to the three existing exact source-C records; actual upstream slots come from the
retained parents. It compares the complete two-page preflight PDF/transcript with
the qualified local candidate and previews the full child closure.

Only after preflight does it refuse a wrong plan pin and a foreign plan absent
from original retained work, then stage/recover the original control's explicit
policy close. Historical replay must use its original 264efd runtime, and the
other Waiting job and every original parent/profile pin must remain valid. It
does not admit or execute the issue task; those controls are pinned for that next
step. Frozen-source qualification is pending.

The first frozen issue preflight at source
`ef9b0ec105313943d4792e08f02ae011dcb4b8dd` passed packet/witness correspondence and
byte equality for the full local PDF/transcript, then refused its premature
slot-admission preview with `NeedsRecovery/native-admission-reconcile-original`
after 73.03 seconds. The old control still occupied that slot. There was no close,
new issue admission, journal or worker invocation. The failed source/tools/logs
remain in `F:/src/tig-dev-temp/reel-corrected-issue-preflight-20261120-v1`.

Declared resource inspection is now separate from admission eligibility:

```text
reel-workflow-native inspect-input-closure PACKET SOURCE ADMISSION ATTEMPT
```

`admission::Prepared::declared_preview` validates the bounded declared resource
envelope and reports its object/byte totals without checking slot readiness,
opening native payloads, accepting protection or creating native work. Existing
`preview` retains its actual slot-admission checks. A new standard fixture proves
declared preview works while another operation occupies the slot and that normal
admission still refuses with unchanged actual Index and original positive claims.

The corrected issue fixture now saves exact preflight controls and verifies the
actual resource-inspection CLI while the old slot is busy. It checks original
Index equality against the retained acknowledgement before any new effects. Only
after explicit original-control policy close does it recheck complete live-parent
custody and slot-admission preview, without accepting the issue yet. That final
preview must equal the earlier declared resource totals. This is not a retry of
an uncertain admission: the first attempt stopped during a dry preview before
creating any new operation.

Producer-applied rights/provenance and comics-editor checklists preserve immutable
lineage and require complete issue context after a local fix. They do not confer
creative, translation, font, voice or release authority. This is generic workflow
work in REEL; no real S5/BBJ asset definitions or active production bindings change.
