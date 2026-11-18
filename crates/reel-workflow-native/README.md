# Native workflow input bridge

This read-only bridge binds a task's exact Git commit, logical records and
explicit annex leaves using Tig's qualified Rust SDK. Its publication
`InputReader` holds verified native streams and executable handles through the
check. It does not accept a durable job actor, stage a manual workspace, publish
outputs, acknowledge retained ownership or apply selections.

Build/check this separate workspace explicitly:

```text
cargo test --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked
cargo clippy --manifest-path crates/reel-workflow-native/Cargo.toml --all-targets --locked -- -D warnings
reel-workflow-native check-snapshot PACKET SNAPSHOT_PROFILE
reel-workflow-native check-publication-native PACKET NATIVE_PROFILE
```

The isolated graph consumes `vendor/tig-workflow`, preserving all 247 source pins
and the original source-pin manifest. It identifies dirty qualified source;
c62c58c ancestry alone is insufficient. The bridge lockfile was seeded from that
SDK lockfile to retain dependency versions. Existing producers still consume
`vendor/tig`. CI checks both workspaces explicitly. This is source integration,
not installation of a qualified lifecycle distribution or the newer four-store
reader. Upstream source bytes remain unchanged, including line endings.

`SnapshotProfile` has schema `reel.workflow-native-snapshot.v1`, pinned Git tool
(`path`, `sha256`, `bytes`), absolute repository, exact commit, explicit record
and definition scopes, complete `bindings` and `input_streams` maps. Every
logical input maps to its selected annex path and all declared stream paths.
Each input choice is explicit: null means selected; a string means that exact
slot. No alias inference, working-tree reads or latest-byte fallback occurs.

`SnapshotProfile::declaration_bytes()` produces canonical portable bytes. Pin
their SHA256, length and JSON media type in `Task.native_inputs`. The declaration
includes commit, scopes, record-to-leaf mappings and stream choices; private
machine tool/repository locators are excluded. Changing a byte-identical leaf
mapping requires a new task key. Preserve those authored bytes alongside the
original task definition. `handoff::Ticket.native_binding` is separate: that
operational admission must preserve accepted work, activation generation,
source/Index checkpoints and owner acknowledgement. Input identity is not job
admission. Existing v2 keys with no native declaration remain unchanged.

`NativeProfile` contains `snapshot`, pinned `annex` tool and the SDK reader's
explicit `source` (`pool`, `store`, `maximum_bytes`). Provider Git/annex paths
must match the pinned tools. This SDK supports one store. Native bytes require
qualified Windows scope; metadata checks also run on Linux. Only declared
inputs may be read. Callers must retain `NativeInputs` throughout their operation.

Bounds: 128 task inputs, 64 MiB declared input bytes, 256 scope paths, 512 metadata
documents / 16 MiB metadata, 1 MiB controls, 256 MiB per executable (stream-hashed)
and 256 read opens. Smaller caller/provider allowances apply. Typed SDK snapshot
errors are normalized; generic provider I/O remains conservative without parsing
error strings. Public diagnostics omit private paths and submitted text.

`check-snapshot` binds actual committed metadata without reading payloads.
`check-publication-native` additionally checks the comic closure using guarded
native reads. Both report durable job protection false. Their output is an
observation, not a lease; guards end when the command returns.

Seven real-Git tests cover historical commits, bare history with unusable original
checkout files, exact leaf/stream identity, declaration changes, tool pins and
CLI authority flags. Another test verifies all 247 SDK pins. An ignored Windows
test explicitly reads an existing synthetic store via `REEL_NATIVE_INPUT_FIXTURE`;
it creates no store or payload. It was run locally against a previously published
1 MiB object, including allowance and undeclared-read refusal. This does not
qualify a complete native comic job, external import or retained publication.

Next: bind these exact resources to a managed job context, accept/reopen its
original actor protection, stage resumable handoffs and implement idempotent
registration/retained-owner recovery through `handoff::Lifecycle`. Completion,
selection and guarded scratch retirement remain separate. Project adoption
requires those fixtures and an owned rollback.
