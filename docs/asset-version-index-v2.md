# Cache-backed version controls and legacy protection reads

`compact-versions` explicitly freezes the unchanged version control into an
authorized, verified active-cache asset and replaces the Git control with a
small compatible `caminos.version-index.v2` header:

```text
reel-assets --root <project> compact-versions
```

The header binds `base_sha256`, `base_bytes`, and an object `patch`. A base is
exactly one typed State control, at most 4 MiB, authorized by that consumer's
registry. Recursive index bases are rejected. E/D reads verify SHA-256/bytes;
corruption fails rather than falling through, and archive roots are never used.
Expanded cache-backed controls share that 4-MiB read bound. The Git descriptor,
its patch file and each immutable version record retain the 256-KiB file bound;
repository snapshot/history limits are unchanged. Writers reject an expanded
state that cannot be read back before publishing a delta. Unknown fields,
invalid required fields and unsupported schemas fail closed.

Existing flat controls and immutable version-record shards remain readable.
Subsequent transactions preserve the cached base and write only changed control
fields. Object members merge, null removes a member, and arrays/scalars replace.
Explicit compaction can freeze a later baseline without changing generation,
locks, selections, pins, leases or approval scope. This is representation
compaction, not media deletion, Git history rewriting or creative selection.
The base and referenced metadata records are protected runtime control roots.

Source copying/registration precede the short metadata lock. Publication checks
that the original control has not changed; on conflict the verified base may
remain retained but the control stays unchanged. Original inputs and all prior
Git versions remain available. Do not raise Git limits to duplicate a large
control index at every transaction.

## Explicit read-only legacy consumers

The redundant-copy request may declare `legacy_consumer_roots`, a subset of its
active consumer roots. This is required for older consumers without a project
profile; absence is never silently interpreted as no protection. The reader
uses the invoking project's explicitly configured active caches to resolve that
consumer's own authorized flat/indexed metadata. It does not create or edit the
consumer's configuration, worktree or registry, and does not probe cold storage.

Current-version/dependency closures, leases, metadata roots and typed catalogue
bindings/cel descriptors protect hashes. Missing/malformed/unauthorized metadata
holds the operation. The original control files, authorizations and catalogue
bindings participate in stale-state checks and Windows control-file guards.

Export an explicit protection snapshot before choosing a batch:

```text
reel-assets --root <project> consumer-protections <request.json> <new-private-export.json>
```

The export records declared consumers, metadata fingerprints and the union of
protected hashes with consumer indices. Git worktree registration and source
controls are checked before completion. It is a point-in-time private report,
not permission to delete; apply independently revalidates protections.

Default `tier-demote` verification may hash copies while jobs are active. It
does not hold the shared mutation lock or remove payload links and reports
admission presence. Actual `--apply` still refuses live/unreconciled admissions
and requires the compatible admission protocol or quiescence. Existing legacy
readers need an explicit tool adoption before they consume a newly compacted
control; source pin updates and ignored binaries are separate operations.
Readers predating the cache read-budget separation reject bases larger than
256 KiB, so adopt the compatible reader before publishing such a baseline.

Fixtures cover exact state preservation, small transactions, corrupt/fallback
bases, unauthorized and recursive imports, required/unknown patch fields,
metadata-root protection and read-only explicit legacy consumers.
They also cover a cache baseline exceeding the Git file limit while preserving
every version, selection and pin, plus rejection of oversized cache/patch state
and oversized ordinary control files.

The actual executable is also exercised as a subprocess: copy verification runs
on its real main-thread stack, with bounded heap hashing buffers, and Windows
apply preserves the verified history copy. This guards against stack failures
hidden by the larger stacks of unit-test worker threads.
