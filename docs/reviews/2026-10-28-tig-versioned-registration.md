# Versioned package registration

Entry 3b084716ba2ecdff50e30edf089316ce568deae7. Tig WP-010/016,
REQ-001/006/010, SPEC-010, IF-010/012, PKG-001/006 native compatibility adapter.
Scope: one bounded immutable artifact-version record to one small Tig candidate,
independent of media family. Reuse the existing record/definition/cache ports.

Preserve the exact logical artifact ID, kind, current package and every retained
version package/lock. Retain the complete original control record as a separately
addressable evidence stream, rather than copying its expanding index into Git.
The original source record must already be available through explicitly enrolled
byte providers. Do not read package/media payloads, discover caches, write caches,
select production outputs, transfer approval or apply Git changes. Fail rather
than omit duplicate, foreign, inconsistent, oversized or unsupported versions.

Expose an explicit --versioned mode on the existing Rust registration CLI. Pin
definition/schema/source bytes and publish a single candidate file without
overwrite. Stdout is counts/booleans; raw errors/profiles stay private. Source
packages remain opaque and unclassified, with production/custody/adoption open.

L0 locked fmt/Clippy/default isolation. L1 multiple families, stable identity,
current/prior pins, original raw source evidence, duplicate/current/lock/URI/size
and bound mismatches. L2 actual CLI on synthetic enrolled metadata, no media reads,
existing-file refusal and immutable input; actual catalog conversion follows
qualification and explicit metadata retention. Producer roles: provenance/privacy
and Git/adoption boundaries; no technical projection is human approval.

## Local qualification

Three new tests pass: eight media/document families preserve identity, current
and prior package pins and raw source evidence; eleven malformed-version cases
plus source/schema/duplicate-key/size refusals fail without silently losing
history; the actual CLI works with PATH empty and only the named small metadata
object present, refusing overwrite and corrupt source without leaking raw paths.
The five existing native registration tests also pass through the unchanged
default CLI mode. Locked all-target/all-feature Clippy (warnings denied),
no-default-feature library isolation, formatting and whitespace checks pass.

The read-only committed CAMINOS catalog survey finds 675 artifact records, the
largest 42,055 bytes; none exceeds the 64 KiB input limit. That is a size survey,
not a claim that all records pass parsing, envelope bounds, package availability,
historical restoration or semantic conversion. Actual retained catalog conversion
and exact-head hosted Linux/Windows/FFmpeg checks remain required.

Producer review: provenance/privacy preserves the full original record by exact
evidence pin and never copies unknown expanding fields into Git. Git/adoption
preserves per-asset identity and original release declarations without granting
approval to new subjects. Package references remain unclassified; no primary
input, custody or eviction claim is inferred. Source-format compatibility lives
in the REEL adapter; the low-level Tig byte providers remain family-neutral.
