# Public Tig consumer integration

REEL's Tig adapters now consume public Tig commit
`0ceade2a16de1c1ed6cc27e48675e7945d7b61df`. The import contains upstream source
blobs, not upstream Git ancestry. All 115 imported files (1,258,310 bytes) match
upstream Git blobs exactly, including the unchanged MIT license. The added
catalogue web files are embedded compile-time dependencies of the upstream crate;
no catalogue service or project is started or enrolled by this change.

The source-pin test now checks the entire import census, including embedded web
assets, as well as repository, license, commit and per-file size/SHA-256. The root
lockfile adds only Tig's already-resolved optional `regex` and `flate2` dependencies.
REEL adapters remain thin consumers; no generic Tig API is reimplemented here.

The [public integration guide](../tig-public-integration.md) provides the feature
map, a synthetic two-version example and project-configuration entry points.
Historical review records retain their original source references; current
history-host and vendor guides identify the new dependency.

## Checklist review

This is a maintainer-applied review using the repository's role checklists, not an
independent approval or simulated reviewer sign-off.

- **Rights and Provenance Steward:** canonical public source and raw hashes are
  explicit; upstream license retained; no private profiles, media, runtime roots
  or Git ancestry imported. Rendering/checking is kept separate from creative
  selection, consent, approval and delivery. No implicit upload or runtime switch.
- **Platform and Audience:** the README leads to one integration entry point;
  features and external tools are explicit. Windows-only mandatory path protection
  is disclosed, with unsupported hosts refusing rather than weakening protection.

No blocking finding remains in these checklist scopes. Media craft approval is
outside this dependency/documentation change; upstream CI alone is insufficient.

## Local verification

- Exact upstream Git-blob comparison for every imported file; source-pin hash and
  census test; integration guide local links; formatting and diff checks pass.
- Locked all-target/all-feature consumer build passes without REEL adapter API
  changes. Gitleaks 8.30.1 reports no leaks in the imported tree.
- The native two-cel/shared-cue CLI fixture passes with real FFmpeg: stale pin
  rejection, changed-cel decoded comparison, unchanged shared audio and archived
  replay with Git unavailable (16.76 seconds).
- The fake episode lifecycle passes: four scenes, one replaced cel, reuse of the
  other scenes, parallel publication, versioned delivery, Git branch/merge and
  verified fixture-cache eviction (113.21 seconds).

The PR's checks track the full regression/Clippy run and hosted Windows/Linux
qualification. These source and fixture checks neither operate on production
caches nor update any project's shared runtime binding.
