# Preserve long external version labels in bounded stream slots

Entry `5fb8e4bae930a9608a933d556211bdad48658781`; source preservation,
Tig WP-005/010 and REQ-001/003/010. A current source record has a valid 145-byte
version label. The registrar admits labels up to 256 bytes, but blindly adding
version/ produces a slot over Tig's unchanged 128-byte bound. The candidate
correctly refused; the source and earlier successful candidates remain retained.

Use the existing version/name slot when its UTF-8 encoding fits. Otherwise use
version-sha256/ plus the full SHA-256 of the exact UTF-8 label, retaining the
original label in provenance.source_version_id. This prefix is disjoint from
all literal version/ slots, including labels resembling hashed addresses. Do
not truncate, normalize Unicode, broaden Tig limits or change package identities.
Public version_slot provides the same deterministic address for consumers.

The authored current_version and version-lock identity retain their original
meanings and bytes. Source-record evidence, selected package, ordered history,
release declarations and unclassified status are unchanged. Ordinary short-label
outputs retain their previous JSON values; only encoded slots gain explicit
source-label provenance. These slots remain logical metadata, never physical paths.

Identity/namespace producer review checks full-label distinction, byte boundaries,
disjoint namespaces and historical lookup. Rights/provenance review requires raw
source preservation and no approval/rights/custody inference. These are producer
lenses, not independent or human acceptance.

L0 fmt/diff/locked Clippy; L1/L2 versioned source and package regressions, exact
raw source pins, distinct labels sharing a long prefix, separate lock IDs,
UTF-8 128-byte boundaries, 256-byte source maximum, malformed labels and unchanged
ordinary slots. Hosted qualification and actual batch completion remain open.

## Incremental source updates

The same catch-up batch includes 37 existing source identities. Optional profile
previous_instance and pure prepare_update bind the exact prior native bytes to
the same definition/schema and asset identity before generating an update. The
prior selected package must be represented in its declared version history.
An unchanged source is a byte-identical retry; contradictory current selection,
authored fields or streams refuse instead of being silently repaired.

On a changed source, retain the old native bytes under previous-record as
evidence/native-record in source-history. Preserve every old stream. Exact
duplicates reuse the slot; conflicting names use retained/ plus the complete
previous-instance SHA-256 and original stream ordinal. Original subjects, roles,
groups and provenance remain unchanged. The cached old record supplies the exact
original slot and all old metadata/status; its bytes must be retained before
adoption. New source status stays unclassified, with no approval inherited from
a different subject. Native 64 KiB/slot/duplicate/size checks still apply.

Fixture acceptance includes same-label replacement, older versions absent from
the new source, exact prior-instance retention, no approval transfer, idempotent
retry and wrong identity/definition/pin/current-selection refusal. This is source
preservation, not native production conversion or independent custody.

Local validation: eleven versioned-registration tests and four package tests
pass, including cached previous-instance CLI verification/corruption/no-clobber
and preservation across repeated updates. Locked all-target/all-feature Clippy
passes with warnings denied; formatting and whitespace checks pass. The original
boundary fixture incorrectly counted version/ as seven bytes; the corrected
120-byte ASCII/UTF-8 labels fill its 128-byte slot exactly. Implementation checks
used actual byte lengths throughout. Failed/intermediate logs remain separate.
The observed source batch has 35 labels needing encoding, with a 177-byte maximum.
Actual batch completion and hosted qualification remain separate.
