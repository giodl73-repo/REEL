# Movie history after job-slot reuse

The ignored `movie_close` fixture continues an existing completed movie under
its original runtime and admission. It prepares and stages close, interrupts the
original handoff at the native Prepared boundary, then reconciles the same close
ticket. An unrelated review task reuses the slot with a new activation. Replaying
the old close must preserve that activation and all current claims; reopening the
old admission must refuse. The original retained movie must still verify, with
unchanged claims, after the source job's activation has gone away.

The replacement selects a synthetic object absent from the movie input closure.
Admission protects this waiting task; the fixture does not execute it or imply
that its payload is available. Every operation root, ticket and outcome is kept.
No selected record changes, payload deletion or scratch retirement occurs.

Qualification passed in 182.98 seconds at source
`97f3c31` using the completed synthetic movie in `.tmpoi9NTI` and its original
frozen runtime. The new preserved controller root is `.tmpnfAbsW`.
Exact fixture/runtime pins, all 1,871 verified source files and original close,
replacement and outcome controls are recorded in
`F:/src/tig-dev-temp/reel-movie-slot-history-20261119-v1/qualified-tools.json`
(SHA256 `be17f5d2435dcb2482aa8d0f68411f28014b844aaa16c56496e27f1571d2ec25`).
All 57 default native tests, the 247 SDK pins, strict Clippy with/without `film`,
formatting and `git diff --check` pass.

The original handoff recovered after the Prepared interruption and closed the
slot. The replacement activation survived replay of the old close; old admission
reopening refused. Retained movie verification passed after reuse and left claims
byte-identical. The replacement remains an admitted, unexecuted waiting task.
This qualifies the bounded existing single-pool Windows/NTFS lifecycle, without
extending the 16-actor or 128-changed-path admission limits.

The fixture requires an explicit
`REEL_COMPLETED_MOVIE_FIXTURE` and optionally writes evidence under
`REEL_MOVIE_CLOSE_EVIDENCE_DIRECTORY`. Do not restart it on an already affected
fixture after uncertainty: reconcile the preserved original close ticket first.

Producer-applied rights/provenance review: this is a synthetic lifecycle check.
Retained history preserves exact versions; slot reuse grants no creative,
translation, performance, selection or release approval. No independent review
is claimed. Partial render/controller recovery and project adoption remain open.
