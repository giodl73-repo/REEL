# Synthetic episode lifecycle

The Rust integration fixture in `tests/support/tig_episode_lifecycle.rs` exercises
the native Tig and REEL paths with generated media. It uses a fresh Git repository,
a local bare remote, and isolated temporary hot, history, archive, and delivery
stores. Git tracks only small instance and definition files. No project media,
existing cache, daemon, cloud account, or runtime binding is touched.

```text
main v1: opening + scene A + scene B + credits, shared cue
  -> protected native render / independent media checks / publication claims
  -> ordered four-second episode / simulated local delivery / commit / push
fix-cel-b: replace one image / commit
  -> recipe comparison: only scene B stale; stale job refuses before byte reads
  -> rebuild B / reuse three scenes / assemble / deliver / push / merge main
  -> version closure diff / dry-run / verified history copy / hot eviction
main v1 snapshot -> resolve the original episode from history
```

Decoded comparisons require identical PCM and identical pictures everywhere except
scene B. Each scene uses the managed job protocol: frozen context, protected
ownership, render, checked claim, output readback, adoption, and ownership release.
The episode checker borrows protected primary-input providers through
`episode_conform::build_with_scene_inputs`. Its scene coverage must be exact;
missing coverage refuses before creating an output. Imported legacy scenes refuse
this interface rather than silently reading ambient primary inputs.

The retention adapter derives primary and control objects from the actual native
scene graphs. Compiled episode masters are outputs, never primary dependencies.
Each version is anchored to the hash of its actual small delivery instance record.
The existing asset accounting, tiering, version-diff and demotion implementations
compute and apply the transition. Eviction commits per object, not atomically for
the whole batch. A deliberately missing final backup must block that object's
removal; earlier removals must each have a verified retained copy. Restoring the
backup and resuming the same journal completes the transition. The old cel, old master and
old delivery record leave the hot store; every current object remains verified.

Publication failure injection interrupts a stream before completion, requires no
final object or acknowledgement, then retries the exact operation and recovers
its acknowledgement from a fresh backend. Two independent publisher instances
share coordination: one pauses inside its input stream while the other must
complete. Waits are bounded and workers are always released and joined.

Run on Windows with Git and FFmpeg available:

```powershell
cargo test --locked --features tig-publication --test tig_inputs fake_episode_lifecycle::fake_episode_runs_v1_then_rebuilds_only_the_changed_scene -- --ignored --exact
```

Windows CI runs this explicit external-tool case. Opening and credits are ordinary
native scene units in this fixture; presentation-template behavior has separate
conform tests. The local bare remote simulates branch/push/merge, not GitHub PR
authorization or cloud delivery. All fixture stores share one volume, explicitly
allowed by its test policy; this proves byte checks and refusal logic, not
independent disaster recovery. Operational inventories and receipts stay outside
the fixture Git repository. Fresh render scratch directories remain local; a full
cache-backed output-bundle adapter and real project cutover remain separate work.

Checklist review applied by the producer, without independent human approval:
Editor: ordered cuts and exact unaffected frames are asserted. Sound Designer:
the shared cue's decoded PCM and episode clock remain exact. Rights and Provenance
Steward: generated content only, no upload, exact source/producer claims, and
simulated delivery explicitly records `creative_approval: false`. These checks
establish technical behavior, not creative selection or release authorization.
