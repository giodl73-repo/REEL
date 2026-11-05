# Scoped stillness decisions in native scene compilation

An authored `reel.scene-delivery-profile.v1` may supply `stillness_decisions`.
The default empty list preserves existing delivery job and picture metadata.
Compiler-version receipts still identify the actual compiler source. Each entry has
`scene_id`, `language`, `attachment_id`, `source_sha256`, `sample_rate`,
`maximum_scene_samples`, `reason` and `decision`.

Only an entry matching the selected scene and language can reach the compiled
delivery job. It must name exactly one selected still attachment with the exact
picture hash. The declared sample rate must match the profile and the complete
scene picture clock, including its closing hold, must fit the explicit maximum.
Duplicate scope keys, missing attachments, changed pictures or clocks, empty
reason/decision references and conflicting existing exceptions refuse before
compiled controls are emitted. Other scenes and languages remain unchanged.

The compiler passes the reason and consumer decision reference through the
existing delivery `stillness_exception` contract. It does not raise the general
composition limit, grant creative approval or authenticate the consumer's
decision. The producer must retain the actual scoped decision separately; human
review and runtime qualification remain distinct gates.
