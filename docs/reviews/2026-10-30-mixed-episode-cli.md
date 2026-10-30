# Mixed scene and presentation episode execution

The bounded CLI fixture now has a second composition: a native opening,
two chapter scenes and native credits. Opening and credits have their own
editable layer, measured cue clock, picture timing, template, source records and
selected masters. One chapter uses compiled controls and the other declared
controls. The ordered-unit master explicitly names all four units.

Both variants use the same retained-result and failure lifecycle assertions.
The source checkout, definitions, old scene jobs and original presentation render
directories are unavailable when the CLI runs. It must resolve the captured
source and existing caches, obtain the matching proof type for every component,
assemble 96 frames and check full picture/PCM/timestamps. Receipt assertions
distinguish the two presentation proofs from the two scene proofs. Missing
presentation coverage refuses before root adoption.

The tests reopen retained output hashes and every parent-context object, check
temporary claims were released after root adoption, and exercise idempotent
finish, stale revisions, foreign source reuse, unsafe retirement refusal and a
retained worker failure. The first complete mixed run passes in 196.37 seconds.
Windows CI explicitly runs both variants; enabled metadata tests alone do not
exercise these native commands.

Final strict locked all-target/all-feature Clippy and formatting pass. Both CLI
variants pass together in 258.30 seconds, including the proof-type and incomplete
coverage checks. Hosted qualification remains required.

Producer-applied editor review requires explicit unit order and correct component
proof types. Sound review requires the continuous measured cue clock and exact
assembled PCM. Provenance review keeps original source/selection identities and
the technical/creative approval boundary. These are checklist findings, not
independent creative or publication approval. All media and stores are synthetic
and temporary; this is tooling qualification, not actual project cutover.

The next operational step is an episode context capture command using the
existing bounded capture infrastructure. The current fixture freezes its parent
context through Rust APIs; a user needs that same verified step through the CLI,
without an ad hoc capture script, before real project acceptance.
