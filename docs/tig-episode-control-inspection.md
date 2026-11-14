# Inspect compiled episode controls

`reel-tig-job inspect-episode-controls PRIVATE_PROFILE` returns the exact compiled
conform manifest already framed in a frozen episode job's metadata. Current
native composition can derive this manifest from authoring controls and verified
components. Its receipt pins the compiled bytes, which can differ from the
authored baseline. Consumers such as `reel-episode-captions` still require that
exact receipt-to-manifest hash match.

The strict private profile is:

```json
{
  "schema": "reel.tig-episode-control-inspection-profile.experimental-1",
  "metadata_root": "/private/existing-metadata",
  "metadata_minimum": {
    "identity": "<trusted store digest>", "generation": 1, "nodes": 100, "bytes": 10000
  },
  "resources": {
    "root": {"sha256": "<original resource-root digest>", "bytes": 155},
    "context": "<original job-identity digest>"
  },
  "recipe": "<independently admitted original recipe digest>",
  "manifest_sha256": "<exact conform manifest hash from the native receipt>",
  "max_control_bytes": 4194304
}
```

Use the original worker's resource selection and recipe, with an admitted
existing-store checkpoint. The metadata reader opens SQLite read-only. Named
lookups verify `job/identity`, its expected recipe pin, and
`input-evidence/current-controls-root`. The command reads the named manifest's
framed identity and exact required chunks, checks their lengths and hashes,
then verifies the reconstructed object against the receipt-admitted hash.
The JSON body must identify the existing episode-conform schema.

The output's `manifest.object` and `manifest.utf8` carry the exact bytes. Preserve
UTF-8 without adding a newline or BOM when staging them for a consumer, and
recheck the exported hash and length. Stdout is **private operational payload**:
it may contain private work identities and references, and belongs in existing
asset storage rather than ordinary Git or a public report.

The caller's control allowance is at most 4 MiB. All tree and body reads together
are capped at 2,048 node requests and 16 MiB of requested node bytes. No global
inventory, cache discovery, primary media read, process launch, source adoption,
store initialization, publication or file write occurs. Errors disclose fixed
stage names only.

This command authenticates supplied metadata and control pins. It does **not**
replay the complete source context or recompute the recipe's semantics. It does
not attest execution, prove production currency, approve media or grant release
or eviction rights. Keep the original successful producer, receipt, retained
output readbacks and human decisions as separate evidence.
