# Verify provided scene outputs without recreating a delivery directory

Entry REEL2a53647 (PR227 qualification remains required). Episode conform execution
currently assumes master/receipt siblings and rechecks native scene deliveries in
local directories. This change ports the existing scene checker output access to
the same borrowed `InputFiles` interface already used for primary sources.

The explicit receipt FileRef is verified by the output provider. Receipt schema,
compiled plan, exact output set/names and every output pin are checked before media
checks. All subsequent picture/audio/overlay/camera/Motioncraft reads use the exact
resolved output map; there is no directory/path fallback. Tool selection stays with
the primary provider. Historical local-root APIs call the same checker and retain
their exact case/containment/hash validation. No alternate decoding algorithm or
weaker producer proof is introduced.

L0 fmt/default isolation/Clippy. L1 missing/corrupt/stale output bindings and exact
receipt/plan/output-set failures. L2 synthetic full native render, then relocate all
outputs to unrelated flat names and independently recheck frames/PCM, overlays and
indexed motion evidence without the old output tree. Exact-head Linux/Windows/
FFmpeg checks before squash. This provider port alone is not a complete Tig output
bundle adapter, native episode executor, current-source producer qualification or
custody/eviction authority. No actual project movie read/render/copy or cache change.

The first relocation fixture exposed an extension-dependent PNG reader in indexed
motion evidence. It now explicitly decodes the required PNG format at the verified
provider path. A repinned PNM carrying identical pixels is rejected, preserving
the format contract instead of guessing from a cache filename. Editing/provenance
checklists (producer-applied, not independent reviewers) retain exact clocks,
pixels, source/tool identity, producer evidence and private logical receipts.

Final local qualification passes: relocated-output/primary-provider synthetic media
fixture (11.48s), including oversized receipt refusal before any provider access,
missing/stale/corrupt outputs, wrong receipt pins/names/set/clock and repinned PNM
format refusal. Returned receipt values remain identical and output providers
cannot choose tools. Scene regressions: 26 pass, 14 explicitly ignored external
cases; independent episode consumption passes (31.97s); presentation/audio
normalization conform passes (13.20s); all seven ordered-unit conformance cases
pass (287.07s), including verified presentation reuse, source-byte mutation and
complete episode rechecks. Default library: 191 pass, 9 explicit external cases
ignored (0.95s). Final all-feature/all-target locked Clippy passes with warnings
denied (45.62s); fmt and whitespace checks pass. Hosted exact-head qualification
is still required before merge. No actual private project media was processed.
