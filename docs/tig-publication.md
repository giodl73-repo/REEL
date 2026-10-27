# Existing-cache publication

Enable `tig-publication` to use `reel::tig_publication::ExistingPublisher` as a
Tig `PublicationBackend`. This optional writer reuses `reel-assets` capacity
admission and its verified CAS writer. The read-only `tig` feature remains
independent. No daemon, cache discovery or project initialization occurs.

Provide a private `Profile` with existing absolute artifact, coordination and
destination directories, an operation namespace, `reel_assets::Config`, verified
reader limits and explicit protection requirements. The destination must be a
configured active cache with an existing `objects/sha256` directory. A missing
cache is refused. Configured root names are arbitrary; there are no drive-letter
rules in this adapter. Other layouts need a different admitted writer.

```text
caller-owned Read + exact Object + owner/operation
                  |
          Tig publish protocol
                  |
      destination/object fence (not whole cache)
                  |
      exact private intent -> capacity reservation
                  |
      existing CAS writer: one bounded staged output
                  |
      verify -> atomic no-clobber link -> synced receipt
                  |
      fresh protected read -> exact completion record
                  |
      ProcessRestart acknowledgement
```

Call `tig_store::publish(&mut publisher, &request, &mut source)` or recover the
same request with `tig_store::recover`. Retain the source until acknowledgement.
Retries check the stored owner, operation, object and acknowledgement level.
Existing valid bytes need no source read; missing previously acknowledged bytes,
corrupt bytes and conflicting records refuse rather than silently repairing.
An interrupted source may leave a protected intent with no acknowledgement;
an exact retry can supply the retained bytes. No extra source staging file is
created. The existing writer still stages its destination before publication.

All cooperating writers must share the enrolled coordination root and agree on
namespace ownership. A destination/object fence excludes competing publication
of that object; different objects can stream concurrently, including in the same
worktree. Capacity and receipt transactions hold only short metadata locks.
Operational records and promotion receipts stay under private configured control
and artifact directories, outside Git. No growing inventory enters the source.

This backend acknowledges at most **ProcessRestart**. It does not claim directory
or power-loss durability, complete job enrollment, independent retained custody,
approval, delivery or permission to retire any source/cache copy. The enrolled
directories must remain under trusted local administration; checking ordinary
directories is not a sandbox against an actor swapping directory paths during a
write. Windows fresh verification holds mutation/rename protection; Unix supports
only explicitly requested cooperative-immutable reads and refuses mandatory mode.

Fixtures cover a 40 MiB stream beyond Tig LocalStore's separate prototype limit,
reopen/retry, short/long/failing sources, corruption and missing acknowledgements,
capacity denial, same-object exclusion and independent overlapping writers.
These fixtures do not activate a live cache or switch a native render workflow.
