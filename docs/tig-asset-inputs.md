# Asset IDs as native REEL inputs

`scene_authoring_inputs::read_verified_alignments_from_assets` reads a scene's
selected phrase alignments through an injected reader. The logical IDs and exact
byte pins already live in `ScopedBindings`; no second path inventory is needed.
This is a Rust library entry point. Existing command-line builds continue using
their explicit alignment path manifests.

```text
scene cue --binding key--> ScopedBindings --logical ID--> captured Tig view
                                 |                            |
                         expected hash/size             selected object
                                 |                            |
                                 +-------- compare -----------+
                                                              |
                                                     existing byte provider
                                                              |
                                                    guarded, bounded read
                                                              |
                                          REEL hash/size + cue/language check
                                                              |
                                            native alignment/event compiler
```

The reader callback receives `&AssetRef` and a maximum byte count. An application
adapter using Tig should:

1. Capture one immutable source view and resolve `asset.logical_id` within it.
2. Use an explicitly admitted definition and payload selector. A selected object
   can be a manifest in another type; it must not be guessed to be timing data.
3. Compare the selected object's SHA-256 and size with the scoped binding before
   reading it. A newer object with the same logical ID is a stale-binding error.
4. Open that exact object through `ByteSource::open_verified` and read at most
   `limit + 1` bytes, keeping the returned guard alive until the read completes.
5. Return those bytes. REEL independently checks their hash and size, parses native
   alignment JSON, and checks cue/language identity.

The application owns this adapter, allowing REEL to remain independent of Git,
Tig deployment paths and cache layouts. REEL adds no private-repository Cargo
dependency. Explicit Tig sources can be Git commit/index views or other admitted
providers; REEL performs no latest-version lookup, network fallback or directory
search. Retained stream slots require an explicit application mapping; they are
not implicitly selected when the current selection is missing or stale.

All requested bindings are preflighted before storage callbacks: selection state,
unique nonempty cue IDs, unambiguous binding, lowercase hash, cache URI and byte
budgets. Limits are 8,192 cues, 4 MiB per alignment and 16 MiB total returned timing
metadata. The callback must enforce its allocation/read bound; arbitrary application
code cannot be sandboxed by a Rust function signature. The built-in check refuses
overlong returns, but cannot undo memory already allocated by a broken callback.

This entry point validates transport identity, not the entire scene. Normal scene
resolution, selected-take/clock validation, producer admission, production and
release gates still apply. It does not render or certify media. Read/seek access
needs no temporary media copies. A later path-based FFmpeg integration must keep
effective path protection alive through the child process and must refuse a
provider that cannot supply that protection. A callback returning bare paths is
not a substitute for that lifetime.

Tests in `scene_authoring_inputs` compare ID and path readers, feed returned data
through the native event-clock compiler, and refuse changed bytes, unavailable
objects, cross-language/cue data, ambiguous/unselected bindings and over-budget
requests. Synthetic tests are not native CAMINOS cutover approval.

Local Windows verification (2026-10-26): `cargo test --lib --all-features --locked`
passed 183 tests with eight existing external-tool cases ignored. Four new tests
cover this entry point. Formatting and all-target all-feature warnings-denied Clippy
passed. Platform CI remains a separate obligation.

An external, temporary Rust rehearsal linked this REEL consumer with Tig
`d754d2e587f2da0041901ed30de3f7cc0c7c42b8` through local source dependencies;
no private Git dependency was added to this repository. A synthetic two-commit
Git fixture changed only one `.tig` record. Its exact admitted definition and
selected pin resolved through `AssetSnapshot`, then `ReadCache` held a bounded
verified read. Native event spans ended at 48,001 samples in the first view and
96,001 after the binding update. An old binding against the new view refused
before byte IO; the retained old view restored 48,001 samples. A missing first
tier was skipped without creation, two tiny synthetic objects remained, and all
reader guards were released. This was local integration evidence, not a checked-in
cross-repository CI fixture or a private episode/render parity run.
