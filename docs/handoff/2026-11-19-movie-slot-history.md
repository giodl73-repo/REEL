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

Qualification is pending. The fixture requires an explicit
`REEL_COMPLETED_MOVIE_FIXTURE` and optionally writes evidence under
`REEL_MOVIE_CLOSE_EVIDENCE_DIRECTORY`. Do not restart it on an already affected
fixture after uncertainty: reconcile the preserved original close ticket first.

Producer-applied rights/provenance review: this is a synthetic lifecycle check.
Retained history preserves exact versions; slot reuse grants no creative,
translation, performance, selection or release approval. No independent review
is claimed. Partial render/controller recovery and project adoption remain open.
