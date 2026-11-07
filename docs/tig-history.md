# Existing-cache history command host

The current dependency is public Tig commit
`0ceade2a16de1c1ed6cc27e48675e7945d7b61df`; see
[source provenance](../vendor/README.md) and the
[consumer workflow](tig-public-integration.md).
Scope: exact vendor source intake and one optional thin host of Tig's history
application service through REEL's existing CAS publisher. No cache engine,
renderer, legacy script, live enrollment or tool binding change.

```text
reel-tig-history check|publish|propose PROFILE REQUEST
        |
Tig: bounded parsing, exact record/schema, source/Git preflight
        |
REEL: admit existing Reel CAS publisher from explicit settings
        |
Tig: acknowledge metadata, paired readback, immutable Git proposal
        |
ordinary reviewed Git application; separate claim handoff
```

Enable the optional tig-history Cargo feature. Use Tig's
tig.history-command.experimental-1 profile/request contracts. Publication layout
must be reel-cas. Settings schema is reel.history-publisher.experimental-1:
artifact_root, namespace, storage (existing reel-assets Config), optional
reader_limits, and protection. The publication root supplies the destination and
must be explicitly enrolled in storage.cache_roots and the reader profile.
Use configured-local-roots for project-neutral placement. Neither host nor service
creates missing cache/control/artifact roots. Existing explicit publication may
create its bounded operational records as before.

Check/no-op never construct a publisher. Settings are data for this compiled host,
not executable hooks. Errors are static; no private roots/settings in stdout.
New metadata is verified through the paired reader before proposal creation.
No media, renderer, upload, Git application, approval or old-root release occurs.
Old source/job protection remains until separately acknowledged claim handoff.

Verification: exact upstream source-pin hashes, Cargo fmt, focused synthetic
history command and existing publication/regression tests, locked checks/Clippy.
Vendor qualification does not install a consumer runtime or change a project's
binding. Consumer Windows/Linux checks must pass before merging dependency updates.
