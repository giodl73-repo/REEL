# Preserve package observations and complete version history

Package projection rejected otherwise supported metadata whose object pins
included advisory placement fields. It also decoded only short version slots,
although the source registrar already admitted longer display labels through
hashed slots. The fix keeps strict admission and shares the source registrar's
version-slot encoding rather than rewriting original packages or truncating names.

## Contract and verification

Optional `path` and `reused_active_object` fields are typed observations, retained
only in original package evidence. A path is bounded to 4,096 UTF-8 bytes without
control characters. Neither field controls resolution or asserts byte presence.
Malformed observations and unknown fields refuse before candidate publication.
The positive fixture uses a nonexistent private path and absent payload; output
contains neither that path nor the reuse flag and remains unclassified.

Long display labels retain their full identity separately from immutable lock
IDs. Parent and leaf history use the shared slot encoding; hashed slots carry
the full label in provenance and must agree with its digest. Short-label outputs
remain compatible. The regression includes multibyte labels, multiple versions
and display/lock differences rather than merely checking successful parsing.

The baseline reproduces two failures in seven package tests before the fix.
The corrected package suite passes all seven; the native registration suite
passes fourteen and versioned registration passes eleven. All-target,
all-feature Clippy with warnings denied, formatting and whitespace checks pass.
A private metadata-only six-package exercise projects 35 records, including 29
independent file assets; the five packages admitted by the earlier qualified
tool produce identical candidate bytes. It reads no primary media and is not
complete family, publication, native-production or custody acceptance.

## Producer review

Identity/history risk is high if an adapter drops or truncates versions: the
shared encoding and explicit full-label/lock regression address this scope.
Provider/privacy risk is high if an observed path becomes an authority: typed
admission, exact original evidence and the nonexistent-path/no-leak fixture
address that scope. Availability and eviction remain unproven by design.

Rights/provenance and editing checklists preserve original selected subjects,
source evidence and unclassified production state. This is a producer-applied
review, not independent acceptance, creative approval or runtime adoption.
Owner: native registration. Hosted exact-head qualification and actual consumer
acceptance remain separate from these local results.
