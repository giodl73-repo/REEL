# PNG returns for manual image tasks

Image handoffs now share the bounded PNG decoder used by the direct Images API
adapter. The provider-neutral helper lives in `reel-workflow-http::raster`; API
request composition/normalization still lives in `openai_image`. Moving the helper
adds no transport or storage engine.

```text
reel-workflow-native pack-image-result PACKET NATIVE_PROFILE ADMISSION_PROFILE RECEIPT RETURNS NEW_RESULT --apply
```

This command reopens the original active input claims and reads explicitly named
caller-owned return files under ordinary file/directory guards. `RETURNS` uses the
existing `reel.workflow-returns.v1` schema, for example:

```json
{
  "schema": "reel.workflow-returns.v1",
  "root": "F:/owned-return-directory",
  "files": {
    "one.png": "images/one.png",
    "two.png": "images/two.png"
  }
}
```

Each key must name a declared PNG output. File bytes must fit its contract and
pass complete PNG decoding, CRC/end checks and the existing 64 MiB/16,777,216-pixel
bounds. Multiple PNG outputs are supported within the 128 MiB total return bound.
Relative traversal, reserved paths, case aliases, malformed files and missing
required outputs refuse. Animated PNG remains outside this still-image scope.

The packer hashes exact file bytes, preserves the original task/attempt and writes
a new private result envelope exclusively. It uses `mode: handoff` with unknown
provider/model and no worker receipt. It neither uploads nor generates images,
changes input claims, registers/publishes candidates, acknowledges retention or
completes the task. Partial output writes and all submitted files are preserved.
The existing import/retention operations perform those later boundaries.

Required non-PNG companions refuse rather than disappear. In particular, the
direct API service's required HTTP/config/metadata outputs cannot be fabricated
from a PNG-only return. Choose the manual PNG task contract before admission;
packing is not a mode switch for an uncertain direct API operation. Fixed provider
size/background options are not inferred from prompt prose: this generic handoff
checks its declared byte/type contract and observed PNG structure.

## Import and historical validation

Every native `ImageCandidate` PNG return now receives the same decode checks before
candidate registration. A Waiting handoff performs that preflight before writing
`ImportPending`, so a correctly hashed malformed PNG leaves its journal and claims
unchanged. Direct API normalization retains its additional size/background checks.
Other image formats still require their own normalizers; a declared MIME type is
not format qualification.

Retained result verification also reopens and inspects PNG bytes while holding
their native read gates. PNG aliases are checked before byte deduplication can
skip them. Reads are bounded and preserve exact immutable bytes; they do not
resample, reencode or select a replacement. The same source/output object can
remain a valid reused image without acquiring its gate twice.

The new fixture also reopens a previously retained direct-API candidate under
the shared validator, then drops its guards before starting another lifecycle
operation. It tests malformed pack/refusal and forged malformed import, followed
by valid two-image packing, retained completion and replay under one original
admission. No provider call or invented API provenance is involved.

Applied rights/provenance and animation-direction producer checklists: exact
reference/source identity, private return files, format readiness and unknown
manual provenance stay distinct from image quality, creative approval, likeness
rights, selection or public release. A valid PNG does not prove the prompt was
followed or the artwork is approved.
