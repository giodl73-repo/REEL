# Frozen expansion admission

The controller can now turn a late-bound task template into an exact native
source/admission package without inventing records in the input commit:

```text
reel-workflow-native prepare-expanded PLAN RUN_BINDINGS TEMPLATE SOURCE ADMISSION ATTEMPT NEW_FROZEN --apply
reel-workflow-native accept-expanded PLAN FROZEN FROZEN_SHA256 NEW_RECEIPT --apply
reel-workflow-native recover-expanded PLAN FROZEN FROZEN_SHA256 NEW_RECEIPT --apply
```

Preparation expands exact verified retained selectors, captures their original
packet/result/profile/retained-acknowledgement controls, and freezes the supplied
exact-commit Git profile together with those origins. The configured Git scopes
must cover exactly the fixed inputs; upstream-only work uses an explicitly empty
Git record/definition scope. A prepopulated composite source profile refuses.
There is no automatic latest lookup, checkout modification or payload hydration.

The source declaration is pinned into the concrete task and its key is recomputed.
The new package includes the unchanged prior tasks, run bindings, source profile,
explicit admission actor/operation, attempt and resource preview. Preparation
checks the real configured admission and parent custody without accepting new
claims. It does not allocate actors or assume that available metadata establishes
input payload availability. All existing native limits still apply.

Acceptance requires the exact base plan and frozen-file SHA256, holds the package
file guard, revalidates its correspondence and preview, and rechecks actual
parent custody through the existing composite admission interface before effects.
The receipt is created exclusively and flushed/read back while the original child
claim guard remains held. Source, native/controller and journal namespaces are
refused as output destinations. Failed or partial controls remain preserved.

Recovery verifies the same base/package, task, attempt, source and admission
profile and reconstructs completed original acceptance through the qualified
recovery interface. It creates no replacement operation. Pending/uncertain native
work still requires its existing original-work recovery; this command cannot
turn that state into a fresh attempt. Releasing claims, retiring scratch,
dispatching a provider, selecting assets and completing the task remain separate.

Two added Windows metadata/claims fixtures pass for freezing without changing
claims, exact acceptance and receipt recovery, stale base/packet refusal, altered
preview refusal, wrong package hash, protected output paths and existing-file
preservation. They establish no native payload availability. Strict native Clippy
passes. All 40 default native tests and 57 workflow tests pass. Positive composite
history-to-admission qualification is running separately in
`F:/src/tig-dev-temp/reel-workflow-native-freeze-20261118-v1` against the explicitly
enrolled unused test actor and existing qualified issue history. Its source pin
is commit `070fb66`; all 1,813 archived files match, with manifest SHA256
`128d85952ff0fb6e91cb77b511610bb94feca1e63f04c35c94ded8994979ad85`.
The frozen runtime SHA256 is
`495475ba18c01b2eff26b68eeb1c0df40fb060287c967017beafa7a758e4ae0a`.
These are pins under test; positive composite acceptance/recovery is not yet
reported as qualified.

Producer-applied provenance/recovery checks retain origin/result fingerprints,
original work and activation, explicit actors and preserved failure evidence.
This is neither creative/release approval nor a new cache/retention engine.
