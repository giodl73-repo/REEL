# Check workspace policy before episode source adoption

Real episode preparation exposed a private capacity policy whose operation limit
was smaller than the declared complete workspace. The runner correctly refused
to reserve storage and launched no worker, but its root source had already been
adopted. That obligation remained live; no payload was deleted or silently reset.
The retry uses an explicit new operation and expected source revision, preserving
the previous source history.

Episode intake now validates the capacity policy and requires the workspace
budget to fit its configured operation allowance before runtime/source access,
metadata preparation or journal adoption. This is a static consistency check;
the runner still makes the final transactional free-space reservation, which can
refuse if capacity changes. Output-byte ceilings and other jobs' policies remain
independent. A private operation allowance is not a global cache setting.

Both complete CLI variants include an undersized policy with an intentionally
invalid archive. They assert the policy refusal occurs first and no root source
exists afterward. Full success, retained-failure, unsafe-retirement and ownership
handoff cases still pass. Final local qualification passes strict locked
all-target/all-feature Clippy, both CLI fixtures in 242.43 seconds, formatting and
whitespace checks. Hosted qualification remains required.

Producer-applied provenance review preserves source history, explicit limits and
the separation between static checks and live reservation. These are checklist
findings, not independent production approval. Actual managed episode acceptance,
output-record/Git integration and consumer cutover remain open.
