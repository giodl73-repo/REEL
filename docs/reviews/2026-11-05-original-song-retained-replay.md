# Retained original-song generation replay

The materializer checks the final repair against the admitted normalized patches
and protects original sample bits outside the masks. It cannot establish that
those patches correspond to retained raw provider output. A replacement patch
and a newly materialized recording can pass that narrower core check.

The optional provider entry adds `check-generation` as a read-only retained-byte
operation. It reconstructs conditioning, request parameters and normalized audio
from the exact original, controls and retained raw attempts, then checks the core
result and complete generation receipt. Installed model, script and Python paths
are historical metadata here; replay performs no provider call, setup, retry,
selection or publication. Original controls and the retained result root must be
independently admitted by the caller through its managed retention entry.

Producer-applied technical review:

| Lens | Finding and disposition |
| --- | --- |
| Reconstruction | Require exact raw-to-normalized bytes and independently checked protected original samples; disclose both cropped excess and original-only missing tails |
| Editing and sound | Preserving clocks and bytes does not establish acoustic or beat continuity; both remain unreviewed and listening remains pending |
| Lyrics | Exact canonical line/request correspondence is supplied-word evidence; replay keeps performed-word verification false |
| Rights and provenance | Original private restrictions and exact execution pins remain required; historical process execution, model custody, selection and approval are not inferred from receipts |

Synthetic adversarial coverage includes a changed normalized patch that passes
the core but fails raw replay; twelve raw/conditioning/control/receipt mutations;
and short-tail normalization disclosure. A relocated retained view reopens with
the installed synthetic runtime/model/scripts absent and payloads held read-only.
The explicitly enrolled Windows synthetic child also replays its moved output;
its recorded execution flag stays distinct from independently proven history.
No actual song, musical approval or production-lane adoption is claimed.

Local validation passes 383 all-feature library tests (16 explicit ignores),
the explicitly enrolled Windows synthetic child, all-target/all-feature clippy,
formatting and diff checks. The checker opens no installed model during replay;
the real family recording has not been regenerated or selected by this change.
