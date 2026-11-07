# Check a retained delivery candidate without rendering it again

`reel-tig-job check-retained-delivery PROFILE NEW_REPORT` reopens an explicitly
admitted frozen delivery-template context and checks an exact retained output
packet against its source. It uses the existing delivery lowerer, guarded byte
providers and full native picture/audio checker. No second renderer or receipt
grammar is introduced.

This also supports candidates preserved after an interrupted or failed job. For
example, a completed native check can be followed by a wrapper refusal because an
intermediate exceeds its admitted output ceiling. This command does not relax
that ceiling, convert the failed job into success, issue a producer claim, adopt
source ownership, publish media or authorize scratch removal. Candidate bytes must
already be retained through an explicit recovery operation. Failed-job provenance
and remaining owner/workspace obligations stay separate.

The private profile is `reel.tig-retained-delivery-check.experimental-1` with:

| Field | Explicit admission |
| --- | --- |
| `metadata_root`, `metadata_minimum` | Existing frozen metadata store and trusted checkpoint |
| `source`, `context`, `request` | Exact archived source profile, context selection and `reel.tig-delivery-template.experimental-1` request |
| `producer`, `recipe` | Original declared runtime-manifest Object and delivery recipe fingerprint |
| `receipt`, `outputs` | Exact receipt Object and independent logical-name-to-Object media map, excluding `receipt.json` |
| `byte_source` | Existing cache enrollment with mandatory mutation prevention |
| `runtime` | Current checker host identity, FFmpeg/FFprobe pins and dependencies |
| `cache`, `preparation_cache` | Existing shared coordination and admitted active scratch root |
| `max_output_bytes` | Bound covering the receipt and all admitted media outputs |

The archived source byte provider must also require mutation prevention. Runtime
is hash-guarded independently of the original producer admission. The frozen
recipe must match that original producer, and the complete output set must match both the
freshly lowered source and the retained receipt. Guards cover every source and
output throughout the synchronous check. Missing, corrupt, substituted, oversized
or stale inputs refuse. A report must be new, private and on the admitted scratch
volume; errors disclose only fixed stage names.

The report records exact source/context/recipe/receipt/output pins, the current validator, native clocks
and `native_checked:true`. It explicitly records no execution attestation,
production claim, production authorization, transferred approval or eviction
authority. It does not establish current-source selection, human acceptance,
phone playback or remote delivery. Source Git can remain offline.

The native delivery fixture exercises both ordinary and explicitly admitted draft
sources, offline replay, full checks, report overwrite refusal, wrong recipe,
producer, context, request, output map and byte bounds, corrupt retained media and
unknown claim/request fields. Metadata and source-owner journal checkpoints remain
unchanged. This is producer-applied identity/privacy/retention review, separate
from human creative approval.
