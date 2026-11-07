# Preview native capture metadata demand

```text
reel-tig-job inspect-capture-budget PRIVATE_CAPTURE_PROFILE NEW_PRIVATE_REPORT
```

This diagnostic accepts the existing scene, episode, browser or delivery capture
profile. It runs the same native freeze algorithms over a bounded in-memory
overlay of the declared existing read-only metadata store. Existing nodes are
reopened and verified; repeated new nodes are counted once. Per-node and memory
bounds remain enforced. Scene control compilation uses the existing admitted
temporary preparation mechanism; no media producer runs.

The private report records the exact profile, source checkpoint, baseline store
witness and limits, new unique node/byte demand, and whether those two logical
guards suffice. Its context and archive pins are transient: no metadata nodes
or captured context are persisted. A changed metadata checkpoint or configuration
during the preview refuses publication. Reports must be new files outside the
metadata, cache-object and coordination namespaces on the declared volume.

This estimates the capture freeze, not subsequent child-job preparation or full
episode assembly. It verifies neither database/physical capacity nor component
media. It reserves no capacity, adopts no source, changes no limit, grants no
production/approval/eviction authority, and cannot replace ordinary capture or
execution admission. A successful logical preview remains subject to concurrent
changes and the real operation's complete checks.
