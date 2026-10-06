# Motioncraft layer expectations

`reel-scene-layer-cadence` validates timed alpha-video overlays and the supported final ASS presentation independently of camera holds. This is an engineering check, not creative approval. Post-compose cameras require separate analysis and return an inconclusive result.

```text
reel-scene-layer-cadence job.json --asset-root assets --render-root render --expectations expectations.json --output new-layer-report.json
```

Bind expectations to the SHA-256 of the exact job, native render receipt and selected carrier. Declare every rendered layer exactly once. Each layer's contiguous, half-open intervals must cover its exact native active span. Frame indices use the full scene timeline; carrier decoding starts at the corresponding local frame. Regions use normalized full-output coordinates. Regions larger than 2,073,600 pixels are rejected; split inspection intervals cannot leave native frames uncovered.

```json
{
  "schema": "reel.motioncraft-layer-expectations.v1",
  "job_sha256": "<exact job SHA-256>",
  "render_receipt_sha256": "<exact render/receipt.json SHA-256>",
  "layers": [{
    "attachment_id": "rain",
    "source_sha256": "<selected alpha carrier SHA-256>",
    "intervals": [{
      "start_frame": 12,
      "end_frame": 37,
      "kind": "moving",
      "region": {"x": 0, "y": 0, "width": 1, "height": 1}
    }]
  }]
}
```

Supported expectations are `moving`, `hold` and `occluded`. The validator streams selected carriers and native intermediate composites. It independently checks alpha composition at every later stage in the inspected region, including identity outside later layers' active spans. It also checks that each layer leaves its inspected region unchanged before and after its own active span. Source motion uses premultiplied color and alpha differences. Visible motion weights those differences by transmission through later layers, using the smaller transmission across adjacent frames so movement of an occluder cannot earn motion credit for the inspected layer.

Visibility is evaluated per frame. Insufficient surviving alpha or contribution contrast makes the interval inconclusive, even if other frames are visible. A fully hidden layer cannot earn a temporal pass. A declared occlusion contradicted by visible contribution fails. Composition or timing mismatches fail before visibility can excuse them. One-frame intervals are inconclusive for temporal cadence.

For ASS, the analyzer replays the selected presentation on black and white backgrounds to infer its color and opacity response. Replay uses the preceding native picture timestamps, the selected font directory when bound, and the recorded FFmpeg version. The full delivery check verifies the source, fonts and render before analysis. This supports a single ASS presentation or timed video overlays followed by one final ASS presentation. Text-state boundaries belong in separate expectation intervals; changing text cannot provide motion credit to a frozen effect beneath it. Color-space rounding remains subject to the same composition tolerance, and unsupported or inaccurate composition never earns a pass.

Reports preserve analyzer executable identity, hash and version, source and render bindings, native frame bounds, cadence fractions, visibility and composition measurements. An inconclusive layer has `passed: null`; the aggregate passes only when all layers pass. The CLI retains a failed or inconclusive report and exits unsuccessfully. Existing reports are never overwritten.

The current composition tolerance is four decoded 8-bit RGB channel levels per independently checked stage. Cadence thresholds retain the established native Motioncraft constants. These limits require real-media regression evidence and do not establish universal compositing support. Scope is each declared inspection region; authors must choose regions containing the motion they intend to validate.
