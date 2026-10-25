# Bounded cel editor export review

Producer-applied Editor and Rights and Provenance checklists, plus resource/I/O
review. Scope: export preflight/deduplication and captured alignment identity,
based on producer checkpoint `8c0d99c`. No human creative approval is inferred.

- Corrected: copy limits were checked after media hashing and reused pictures
  could be verified once per event. Metadata-only preflight now validates/deduplicates
  identities and accounts for each media destination before resolution. The
  1000-reference fixture calls its resolver once for each of two unique hashes.
- Corrected: a movie/picture sharing an identity still creates two working copies.
  Both count. Conflicting byte declarations reject before I/O.
- Corrected: file growth could escape the declared copy/verification allowance.
  Handle-size checks and bounded readers enforce the declared extent; copied
  bytes are freshly hashed. An actual fixture proves editing an exported picture
  leaves its canonical source unchanged. Failed exports may require explicit
  disposal of their incomplete owned directory; no automatic cleanup is added.
- Corrected: apply reopened alignment after intake. It now uses the captured
  exact source hash, preserving the optimistic expected-input check.

Eight Rust sorter tests, four unchanged JS tests, strict sorter Clippy,
formatting, ROLES structure and diff checks pass. All new media are synthetic
temporary files. This does not extend the one-language/full-recording scope,
certify hostile/reparse filesystems, reserve physical disk space, change shared
runtime pins, replace native build validation or complete general episode editing.
