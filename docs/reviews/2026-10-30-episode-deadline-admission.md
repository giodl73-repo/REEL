# Explicit deadlines for bounded episode jobs

The first real episode preparation reached runner admission but refused its
explicit six-hour step: the episode profile permitted that duration while the
shared runner imposed a fixed one-hour cap. Preparation wrote only bounded
metadata and never adopted the root or launched a media worker.

Fresh trusted runner requests now have an optional
`step_timeout_limit_seconds`, defaulting to one hour and bounded at 24 hours.
Each step must fit its declared limit. The episode coordinator declares exactly
its admitted timeout and validates against the same shared maximum. The process
tree still uses the actual step deadline; disk/publication reservations remain
independent. Existing hash-bound recovery-window rules are unchanged.

The structural test refuses a long step without an explicit limit, accepts a
six-hour declaration, rejects an over-limit step and zero/overflowing limits,
and checks the exact maximum. The complete episode fixture reopens the retained
runner request and verifies its declared limit and actual deadline equal the
episode admission. Existing containment/deadline tests continue to cover process
termination; the test does not wait hours to qualify a long numeric allowance.

Local qualification passes all 316 asset-engine tests, strict locked
all-target lint for that crate and all-feature/all-target lint for REEL, plus
both complete episode CLI fixtures in 274.44 seconds. Formatting and whitespace
checks pass. Hosted qualification remains required.

Real profile preparation also exposed historical scene requests whose delivery
control definitions and language-specific profile slots had changed. The private
requests now admit the current authored-parameter definition and select the
current Spanish profile slots. No source selection, retained master or creative
approval was changed to accommodate admission.

Producer-applied provenance review keeps explicit per-job bounds and source
contracts; editor/sound review leaves content checks with native comparisons.
These are checklist findings, not independent approval. Real managed execution,
output-record/Git handoff, generation/text freshness and full consumer cutover
remain open until independently verified.
