# Explicit existing-cache file publication

Entry REEL `4884d560853c2822d2bd9a984fee7162dee08973` (PR224 qualification
remains required), Tig WP-010/011/016 and existing publication interface. Actual
source reconciliation needs a revised small scene JSON asset. Reuse the existing
publisher/admission protocol; do not write CAS paths directly or embed a large
scene body in Git to avoid the writer.

Scope: an explicit `reel-tig-job publish-file` frontend over ExistingPublisher.
Bounded strict private enrollment, exact object/owner/operation, existing cache
and coordination roots, protected source file, capacity-admitted verified
publication, exact retry/reopen and a new private receipt. No root initialization,
discovery, transfer, render, Git edit, approval, automatic source retirement or
independent custody claim. The caller keeps source bytes until separately
qualified retention permits retirement. Completion of publication is not complete
job/source enrollment or power-loss durability.

Local qualification: five file-publication CLI cases pass (2.44 seconds), covering
exact bytes, reopening, retry after caller source removal, receipt conflicts,
unknown profile fields, hash/length/budget/durability/root refusal, stale owner,
missing acknowledged bytes and an unrelated offline root. All eight existing
publisher cases pass (9.80 seconds), including independent concurrent streams,
object fencing and interrupted-source recovery. Locked all-target/all-feature
warnings-denied Clippy passes (57.03 seconds); default library isolation passes
191 cases with nine explicit external cases ignored (1.12 seconds). No production
source edits follow qualification. Actual small metadata publication and hosted
qualification remain pending; no availability or retention is inferred from tests.

L0 fmt/default isolation/Clippy. L1 exact bytes/reopen/retry without source,
invalid source/profile/roots/budgets, existing receipt and operation conflicts.
Backend concurrency/fencing remain covered by existing publisher tests. L2 bounded
actual scene metadata publication only after explicit enrollment validation;
exact-head Linux/Windows/FFmpeg required before squash merge. Producer
rights/provenance and Git/adoption checks keep availability, ownership, source
selection and creative approval distinct. All raw profiles/receipts stay private.
