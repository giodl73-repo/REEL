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

## Native result and evidence

The positive native fixture passed in 60.62 test seconds against retained movie
job `.tmpy4yMWg`, using preserved staging root `.tmp7f0mI1`. It reopened the
original admission without admitting another job, exported 10 synthetic inputs
and 4 controls, and replayed with the exact original ticket. The workspace's
3,272,447 logical bytes include both parts and linked targets; this is not an
episode-scale byte count. Direct native stream checks retained all ten native
gates while leaving the capture map empty. Source claims and ticket bytes
remained identical. Capacity and scratch remain preserved. No rendering, native
publication, asset selection or consumer adoption occurred. A dedicated native
duplicate-object alias fixture remains open; the positive movie inputs are
distinct objects.

All 69 standard native tests (including the three new component tests), 247 SDK
source pins and strict all-target Clippy with/without film pass. Production source
is `3146ade8f84fbf16d53591e511a0f23ebc67e263`; the fixture-only corrected consumer
is pinned at `f6b1cc3ff5b5a516b32756f2157d1a38cfc3356e`. All 1,932 tracked source
files are archived and individually hash/length verified against build inputs.
Frozen test executable: SHA256
`e896d31aace77632f530971bf3de418c90b5f6f151505eeca8eb103a57aa9423`,
27,963,904 bytes, test profile without debug information, with film support.

Private evidence certificate:
`F:/src/tig-dev-temp/reel-streamed-staging-20261119-v2/qualified-tools-v2.json`,
SHA256 `d922a68c4a7a18d655a4eecf05a0dde7aed82a6e235d8f70cafdf0b54353a281`.
It binds source archive/manifest, frozen tool, terminal log/timing, outcome,
original ticket/pin and intent/completion evidence. It supersedes the first
certificate, which omitted two hashes because of mixed separators in an extended
Windows path; the incomplete certificate is preserved as evidence. The native
fixture was not rerun for that metadata correction.

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
