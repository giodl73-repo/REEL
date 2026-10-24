# Reopen a locked artifact in a new working folder

```text
reel-assets --root <project> artifact-export-plan export.json
reel-assets --root <project> artifact-export export.json
```

These explicit commands copy selected roles from one registered immutable
artifact package. They do not discover files, follow package references,
advance selections, acquire working-set leases, hydrate a checkout, start an
editor or promote media between caches. The same contract works for controls,
recordings, pictures, documents and software assets.

```json
{
  "schema": "reel.artifact-export.v1",
  "record": "songs/example/artifact.json",
  "version_id": "golden-v1",
  "manifest_sha256": "<exact package SHA-256>",
  "output": "/absolute/existing-parent/new-review",
  "max_copy_bytes": 67108864,
  "files": [
    {"role": "recording", "path": "recording.wav"}
  ]
}
```

Use the native absolute path syntax of the operating system. The output parent
must exist and be local, with regular directory ancestors. The output directory
must be new and outside configured cache roots and Git internals. Target names
must be portable relative files: traversal, device names, alternate streams,
case-insensitive duplicates and file/directory overlaps reject. The root
`artifact-export-receipt.json` name is reserved.

Both the version and manifest hash are mandatory. An older, unselected version
can be reopened without changing the current record. Unknown roles and stale
locks reject. Every selected identity must remain registered and protected by
that version's package contract; references can be copied when explicitly
listed, but do not become build dependencies or current residency roots.

Registry/package metadata use the project's existing configured caches.
Payload reads default to the same active roots. Optional `read_roots` specifies
1–16 absolute local payload roots for this invocation under the project's
existing path policy; it changes no persistent configuration. Roots are tried
in order for each exact object. Absence may fall through; corruption stops.
Archive roots are never probed automatically. Producers must explicitly adopt
their applicable storage profile; the command cannot infer tier policy from
drive letters or from an old receipt.

The request is bounded to 128 KiB and 512 mappings, and the payload-copy budget
to 1 GiB, with the existing per-object read limit still applying. Every actual
destination counts against the budget, including repeated copies of one hash.
Unique sources are freshly verified before creating the working folder, and
each output is hashed again during a bounded copy. Working files are independent
copies, never writable hardlinks to canonical objects. Metadata receipts are
bounded separately to 256 KiB.

The plan emits private JSON to stdout and writes nothing. It is evidence for
review, not future authorization: export reparses the request, checks the exact
version/package again and freshly verifies sources. Cache activity can race with
an export; missing or changed bytes fail rather than establishing a custody
guarantee. No global cache lock is introduced.

On success, export writes a private `reel.artifact-export-receipt.v1` containing
the request/package hashes and each role, destination and verified identity.
The receipt is written after all copies. I/O failure or interruption may leave
a partial new working directory without a completion receipt; reconcile it
explicitly. Existing directories refuse even when their bytes match. This is
working-copy transport, not native compiler/render or human approval evidence.
Retain plans/receipts as project cache/LFS assets and apply the appropriate native
validator before using restored controls. Historical frontend exports require
their matching versioned editor host.
