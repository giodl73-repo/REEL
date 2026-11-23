# Owned selection transaction primitive

Author-applied Editor and Rights and Provenance checklists, not independent
review or production approval.

This source slice supplies the expected-ref transaction missing from the
qualified SDK's detached proposals. It deliberately exposes only a Rust
primitive for an explicit owned selection namespace, with no film selection
command or automatic profile adoption. The caller supplies exact old record
bytes, replacement bytes, base, decision pin and operation identity. The SDK
checks bounded existing regular paths and exact base bytes; the pinned Git
executable applies one expected-object-ID ref change without dereferencing it.

Separate intent and completion refs are necessary: multi-ref Git transactions
on the files backend are not treated as crash-atomic durable operation records.
Only the successful intent creator initiates CAS. Successful CAS or exact
selected-commit readback permits completion; intent by itself grants neither.
Recovery of an unconfirmed intent with a different current value stops, even
when that value matches the original expected base (possible intervening revert).
A confirmed old operation reports historical application and never reselects it.
No failed or ambiguous operation is restarted with fresh identity automatically.

- Editor: stable story/cel IDs, picture ordering, cue timing and affected
  language rebuilds remain the next adapter's checks. This text-only fixture
  does not stand in for an actual corrected-film walkthrough.
- Rights/Provenance: decision pins and Git author identities are lineage,
  not creative or rights authority. Old approvals remain immutable and do not
  approve replacement subjects. Native candidate retained guards, exact annex
  record/leaf binding, authority verification and project adoption are not
  implemented by this generic Git primitive. No CLI exposes it as film selection.
- Namespace: branches, worktree/index files, remote refs and active profiles
  are outside the accepted target namespace. Hooks and fetch are suppressed.
  Observed direct refs are required. Expected-OID CAS does not detect arbitrary
  outside ref-type substitutions or ABA; `--no-deref` prevents such substitutions
  from redirecting writes to a branch. Owned namespace coordination is required.

Validation covers actual local synthetic Git proposals/CAS, concurrent candidates,
read-only preview, unchanged HEAD/index/worktree, hook suppression, exact-byte and
branch-scope refusal, normal/dangling symbolic refusal, changed-operation conflict,
historical replay and interrupted-completion reconciliation/refusal. These are
explicit state-loss fixtures, not process-kill or power-loss qualification.
