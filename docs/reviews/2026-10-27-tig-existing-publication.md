# Existing-cache publication adapter contract

Entry REEL 91cf06070217960f6bdc02414e3979c2eac807d1, qualified Tig runtime
e3d80bb (unchanged vendor). Continues WP-004/005/008/011/012,
REQ-001/003/004/006/007/008/010, SPEC-003/004/005/007/009/011/012,
IF-004/005/009/010/012, PKG-002/003/004/006/008. Scope: optional Tig publication
backend over the EXISTING REEL CAS writer/admission policy, a reusable stream
entry into that writer, bounded local operation records and synthetic fixtures.
No data-cache initialization, actual media transfer, selection, release, removal,
daemon, runtime install or production cutover. Operational metadata remains private.

Reuse the existing writer's bounded source copy, verified atomic no-clobber hard
link, sync and cleanup; do not copy streams into an extra staging input file.
Preserve existing file-source receipts. The new generic reader source records
its source kind without inventing a filename, production evidence or authority.

The adapter takes explicit existing artifact/control/cache roots, namespace,
capacity policy and verified-reader limits/protection. Require an already enrolled
CAS objects/sha256 directory, configured active destination and shared coordination
root; create operational metadata and new shard directories only during explicit
publication. Other layouts, implicit archive probing and ProviderDurable refuse.
Do not change Tig LocalStore's independent 8MiB prototype cap.

Implement Tig's PublicationBackend with scoped destination/object fences,
atomic exact-operation intent/completion records, fresh held existing-byte reads,
configured capacity admission and existing writer calls. Expose at most
ProcessRestart acknowledgement, not power-loss/directory durability or retained
custody. Same request retry/reopen is idempotent; wrong owner/object/level for an
operation refuses. Missing previously completed bytes or present corruption
must not silently republish or fall through to another cache. Source stays caller
owned until acknowledgement and independent custody is still required to retire it.

L0: fmt, warnings-denied Clippy, default/read-only Tig feature independence,
vendor/diff checks and existing provider regressions. L1: missing/misbound roots,
operation conflicts, unsupported levels/layouts, corruption, short/long/failing
streams, capacity denial before data IO, no clobber and bounded records. L2:
synthetic >8MiB media-like bytes into an existing temporary CAS, new provider
process-equivalent reopen/recovery and concurrent independent writers; no extra
source staging copy, metadata outside Git, no live caches or deletion.

Rights/Provenance and Editor producer lenses protect technical-versus-creative
authority and immutable output history. Tig Storage/Custody, Concurrency/Recovery
and Provider/Privacy checklist tensions apply to the provider boundary. These are
producer checklists, not independent reviewers or human acceptance. Native result
enrollment, full job/source/capacity claims, archive custody, retirement and reviewed
adoption remain open; a publication receipt is not an eviction grant.

## Executed scope and producer review

The first four consumer cases exposed an upstream canonical Windows drive-prefix
lookup bug: normal roots verified while std::fs::canonicalize roots returned IO.
The fix lives upstream in Tig PR28, exact candidate
96177878c7db238958c20a5faf1eec9bcfd6f438; the vendor import is raw source from that
commit, not a REEL fork. Upstream qualification remains required before merge.
The fixed writer also canonicalizes normal enrolled roots before comparing shard
locations, avoiding the same spelling-versus-location confusion in the new API.

Windows local execution: eight publication cases pass, including 40MiB streaming,
completed recovery/retry without source reads, wrong owner/object, short/long/IO
source failures, fresh-provider recovery of an incomplete intent, corruption and
missing completed bytes, missing roots/coordination, capacity denial, unsupported
acknowledgement, malformed/duplicate/unknown/oversized records and raw/canonical
writer roots. Holding a same-object fence blocks its competitor; a second thread
finishes a different object while the first stream is paused. No whole-cache or
render reservation is held. Unix has an additional symlink-shard refusal case,
reserved for exact-head Linux CI. A direct writer test covers both Windows root
spellings. The native consumer suite passes 27 normal cases with two explicit
FFmpeg cases left for current-head CI. All reel-assets locked all-target cases,
its warnings-denied Clippy, root all-target/all-feature warnings-denied Clippy,
default-library isolation and read-only Tig library check pass locally.

Producer findings: preserve file-source receipt provenance through the shared
writer and record caller-reader provenance explicitly. Never heal an acknowledged
missing/corrupt object from the caller stream. Completed metadata without its
exact intent is corruption, not an opportunity to recreate ownership. Record
parsing must be bounded and duplicate/unknown-field strict. Ordinary CAS/shard
checks refuse pre-existing redirection; enrolled directories still require
trusted local administration, since this is not an adversarial-directory sandbox.
These are resolved at the experimental provider boundary. Rights/Provenance and
Editor plus Tig storage/concurrency/provider lenses: pass_with_risk, producer
review only. No creative approval, full provider durability, live job/custody
coverage, source retirement, native candidate publication integration or cutover
is inferred. Exact-head Windows/Linux REEL CI and qualified Tig PR28 are required.
