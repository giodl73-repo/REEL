# Streamed native input staging

Owned input export now copies each declared native input through a 64 KiB buffer
into the existing exclusive part/link protocol. It no longer reads and captures
the complete input set into `Vec` values before materialization. Exact native
snapshot/record/annex binding, original admission, capacity reservation, pinned
ticket, owned namespace and completion checks remain mandatory.

The native stream port retains each verified native object gate until its
provider drops; streamed payloads are not inserted in its buffered capture map.
Producer semantic/control binding runs before input materialization and may
still use the existing bounded metadata/format decoders. Duplicate logical
inputs sharing one native object reuse the first verified, guarded local copy;
they do not reopen the same gated native object. All authored identities stay
bound to their own exact snapshot records.

The part writer handles short reads and `Interrupted`, checks exact length,
EOF and SHA256 before linking a target, and syncs the part before the existing
publication hook. Truncated, trailing, corrupt or failed streams leave parts
for original-ticket recovery and cannot publish completion. Resume verifies
existing parts/targets and consumes/checks the source again without rewriting
completed bytes. The four-attempt bound and logical part/link capacity accounting
remain unchanged. Controls and explicitly buffered producers retain their
existing APIs; this does not claim every producer is free of media buffers.

Three generated-stream component tests add short-read/interruption and 64 KiB
buffer assertions, partial-I/O preservation/resume, completed-source rechecking,
and truncation/trailing/corruption refusal. All ten staging component tests pass.
An opt-in native fixture is prepared against the existing retained synthetic
movie job from completed-render recovery qualification. It must reopen that
original admission, prepare/preserve one owned staging ticket, export/replay,
check retained stream gates and an empty capture map, and preserve ticket/claims.
It admits no new native job and publishes/selects no asset. Its original scratch
and capacity are preserved; the actual source/tool/result pins follow execution.

The first run at source `3146ade` used the separate slot-replacement job, whose
admission had qualified slot reuse without qualifying input availability. Native
export returned `NeedsRecovery/native-input-io` after 7.56 seconds and refused
completion. Preserved root `.tmpti9z7R` contains its original ticket, partial
controls and reserved capacity. No input was acquired/published to make it pass.
The positive fixture now targets the existing movie's previously verified native
input closure instead. This is a different existing job, not a fresh admission
or an effect retry for the refused replacement input.

This removes input-export whole-set buffering. It does not raise the 512 MiB
staging reservation, task/actor/path limits, film output/workspace/time limits,
or qualify E7/S5 multi-GB production. A versioned large-film/composition contract,
native lifecycle decomposition and real-size interruption cases remain required.
The fixture is small synthetic evidence, not owned Melena adoption or approval.

Producer-applied editor and rights/provenance checklists preserve exact input
lineage, unchanged review identities, private-only operational controls and
technical/creative authority separation. No independent review, translation,
voice consent, creative approval or release authority is inferred.
