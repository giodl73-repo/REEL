# Sample-clock drafts from Tig inputs

```text
Git commit + admitted definitions
             |
             v
Tig IDs + exact byte pins --> selected-input comparison
             |                         |
             |                  protected readers
             v                         |
       native sampler <----------------+
             |
       PCM stems + check
             |
  private source correspondence --> retain/register separately
```

Build `reel-tig-sample-clock` with feature `tig`. Run it explicitly:

```text
reel-tig-sample-clock --profile SOURCE_PROFILE REQUEST \
  --output-dir NEW_ABSOLUTE_DIRECTORY --max-output-bytes BYTES
```

The source profile is the existing `reel.tig-input-profile.experimental-1`:
exact Git commit, scoped records/definitions, exact definition admissions and
explicit existing stores. Selected-payload admissions are required. The request
uses the [sample-clock contract](music-sample-clock.md), but its input `path`
fields name Tig IDs, never physical cache paths. Each requested SHA-256/length
must equal the selected subject in that captured source. There is no latest
lookup, package extraction, implicit download or historical-checkout dependency.

Controls and input/output bounds are checked before media-provider reads. The
declared output allowance covers all PCM files and a conservative 4-MiB metadata
reserve. Existing Tig readers hold mandatory path protection through render and
the read-only sample checker; this entry refuses on platforms without that
capability. Current support therefore requires Windows. It neither copies inputs
to a worktree nor changes a shared cache lock. Output must be a new ordinary
directory outside configured provider roots and Git internals.

The directory contains the native stems/receipt plus `tig-request.json` (exact
logical request), `render-request.json` (private provider paths) and a final
`tig-source.json` packet. The final packet records the source checkpoint, exact
record/definition/subject pins, both request hashes, render-receipt hash and checker
coverage. Keep these private, cache-backed evidence streams on the resulting
artifact; do not commit expanded packets or provider paths to an asset repo.
I/O/check failure may leave a partial owned directory. Its mere presence is not
success; retain only a complete checked packet through the normal asset workflow.

This is a source-bound **draft authoring** entry. Selected input identity is not
production eligibility. It issues no produced claim, job/capacity reservation,
approval, publication, custody or eviction authority. Managed-job enrollment,
source-policy eligibility, provider publication and lane runtime adoption remain
separate requirements; do not install it as a producer from this document alone.

The synthetic Windows fixture proves Tig resolution preserves PCM, final source
correspondence, no-clobber output and refusal of stale pins, insufficient output
budget and a destination inside the cache. Existing sampler tests retain native
clock, vocal and exact stem-sum coverage.

For admitted cache staging, use the existing frozen job runner with the
[file workspace producer](music-sample-clock.md#file-workspaces-and-admitted-jobs).
The parent owns Tig resolution/admission and the file child consumes its exact
staged pins. The private Tig draft entry above keeps its outside-provider-root
guard; a directory or `job.json` alone never supplies source authority.
