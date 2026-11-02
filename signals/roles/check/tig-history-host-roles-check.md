# Existing-cache history host review

Scope: thin optional host, exact upstream source intake and synthetic tests.
Producer-applied rights/provenance and editor lenses, not independent review or
consumer cutover approval. Generic storage/concurrency review remains in the
exact [upstream service review](https://github.com/giodl73-repo/tig/blob/3a1ce2441957023c08efbedeabf725ed5668c81b/docs/reviews/hosted-history-command.md).

- Rights/provenance: no media, prompts, rights scope or source selection is edited.
  Config/roots stay private; the CLI returns private operator pins, not a public
  scorecard. Static errors do not echo settings. Technical success is separate
  from approval, delivery, source adoption and release of old protections.
- Editor: preparation, publication, proposal and application remain separate
  steps. One shared Tig implementation owns the workflow; REEL only constructs
  its already-existing publisher. No shot/cue/timing/rendering behavior is changed
  by this host, and no creative-quality pass is claimed.
- Dependency intake: 87 manifest/license/Rust-source files are byte-checked against
  Tig3a1ce2441957023c08efbedeabf725ed5668c81b, MIT. No local donor edits. The
  optional feature enables its schema interpreter; normal runtime bindings are
  untouched. Upstream Windows qualification is still required before merge.
- Storage: destination must be an enrolled Reel CAS/read place. Open refuses
  missing cache, artifact and coordination roots. Existing object-scoped fences
  and capacity admission remain; no new cache engine or global lock is introduced.

Two actual host-command tests pass: check makes no operational records; proposal
publishes only new metadata into normal CAS, survives reopen/retry, reads back exact
objects and preserves HEAD/current source until explicit Git application. Raw
media bytes are not copied. A no-op creates no proposal; bad settings/missing roots
refuse without creating roots or leaking paths. Existing publication suite: eight
tests pass, including independent writers, scoped fences, corruption refusal,
interrupted retry and streamed large synthetic bytes. Strict all-target/all-feature
Clippy, fmt, diff and all 87 source hashes pass. Full local/hosted regression
qualification is recorded separately; candidate runtime installation remains open.

Disposition: pass with risk for isolated candidate testing. Do not upgrade another
lane or call Melena migrated from these fixture results. T3 claim handoff and real
asset/coverage/rollback acceptance remain required.
