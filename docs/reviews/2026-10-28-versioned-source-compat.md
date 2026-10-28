# Preserve source display versions and absent declarations

Entry 46e82fb8893fff8d2e011df738b2302a3c4bb207. Same bounded native
compatibility adapter, WP-010/016, REQ-001/006/010, SPEC-010, IF-010/012,
PKG-001/006. Real catalog conversion found 646 successful records and 29 refusals.
Eighteen use different display and immutable input-lock version IDs; eleven omit
public_release. Source bytes and declarations must not be rewritten to fit the
first fixture's narrower assumptions.

Preserve display IDs in authored current_version and version slots. Preserve
the original lock version ID in each stream's provenance, checking its
nonempty ID, artifact identity and manifest hash independently of the display
label. Artifact identity and manifest hash are already present in the instance
and stream; the complete original lock and any extra fields remain in the pinned
source record. Avoid duplicating those hashes and IDs in every history entry.
Keep URI/hash/size, duplicate, bounds and missing-current refusals intact.
Preserve absent public_release as absent; validate it as a Boolean when present.
Definitions must make that authored field optional for these sources. Missing
declarations do not confer release authority; production remains unclassified.

Preliminary text BOM inspection used a culture-sensitive comparison and was
incorrect; raw byte inspection found no source BOMs. Do not add BOM handling for
this catalog. L0 locked fmt/Clippy/isolation. L1 explicit alias and absent-value
preservation, invalid/null declaration and malformed lock refusals. L2 rerun the
real candidate conversion from the already retained exact source objects; no
re-ingestion, media IO, source edits or existing-candidate overwrite.
