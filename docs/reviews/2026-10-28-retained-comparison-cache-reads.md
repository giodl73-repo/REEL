# Retained comparison without output staging

The existing `reel-tig-job compare-retained PROFILE NEW_PRIVATE_REPORT` command
now verifies native outputs directly through protected Tig cache readers. It
removes the complete output-copy directory, its second reservation and copy loop.
Only admitted source/control preparation and the bounded private report remain;
no master, stem or picture output is materialized for validation.

Legacy `reel.tig-retained-comparison.experimental-1` profiles retain their required
exact output map and equality checks. New
`reel.tig-retained-comparison.experimental-2` profiles omit `outputs`: the pinned
native receipt is the sole output manifest. An additional map is refused in v2;
omitting the map is refused in v1. The same admitted original claim, source
context, producer, runtime, baseline and output-byte allowance remain required.
Version 2 reports use `reel.tig-retained-comparison-report.experimental-2` and
retain an `output_manifest` receipt pin plus `output_count`, rather than copying
the output inventory into another report. Legacy report maps remain unchanged.

Both modes restore the frozen native recipe, derive the current job/contract,
verify the exact receipt plan/output set and independently decode all required
media through the same full checker. Baseline comparison remains a separate
output relationship, never a canonical primary dependency. Receipt metadata is
bounded to 4 MiB, with duplicate-key rejection and portable exact logical names.
For existing Windows font receipts, only backslash separator spelling is
normalized to the already validated manifest key. Dot, rooted, drive and traversal
aliases remain invalid. New font receipts emit forward-slash paths on every host;
the native font/overlay fixture checks that portability directly.

One ReadCache charges claim, receipt, reference and unique output verification.
The receipt's original guard is retained and reused, rather than charging another
receipt request or creating a fresh cache with reset limits. Bundle admission
accounts already held readers and already charged requests/bytes before opening
output payloads. All guards stay alive through native checks and parity; tools
come only from the admitted runtime. Budget exhaustion remains refusal.

The CLI fixture runs with source Git/records/definitions physically unavailable
and PATH empty. It compares legacy and receipt-derived reports, requires zero
materialized output bytes, and admits exactly the needed shared request/byte
budget. One fewer request or byte refuses. Existing altered context/producer/
receipt/map, missing/corrupt output, report-overwrite and differing-picture cases
remain. Native clock, geometry, frame/PCM and privacy checks are preserved.

Producer-applied Editor, Sound Designer and Rights/Provenance checklists preserve
the full media comparison, explicit baseline and original claimed scope, separate
technical/creative authority, private reports and no actual project/cache changes.
These are not independent or human signoffs. All qualification is synthetic;
complete project adoption, remote custody, authoring freshness and eviction
authority are not inferred from a matching historical comparison.

Local qualification against imported Tig4e80: locked all-target/all-feature
warnings-denied Clippy passes (56.19s); exact source-pin/file census passes;
compiled CLI/source-offline and both receipt profile contracts pass (521.85s);
both managed episode/cache lifecycle cases pass together (135.36s); native
font/overlay mapped pictures and PCM checks pass (14.26s); all-feature library
204 pass with 9 explicit external-tool cases ignored (1.56s). Hosted exact-head
Linux/Windows/FFmpeg qualification remains required before integration merge.
