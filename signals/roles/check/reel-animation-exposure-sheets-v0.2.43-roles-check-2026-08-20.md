# REEL v0.2.43 animation exposure sheets — roles check

## Frame

**Working owner systems:** BERTICA already owns measured shot timing, anchor
cels, narration cues, and sprite intent. KARTS already owns frame-exact
choreography, pose catalogs, four-frame stride holds, and protected action
ranges.

**Missing shared capability:** REEL cannot currently validate the exact
drawing/pose/effect exposure selected for every frame. Choreography owns motion
phrases and KARTS separately maintains render-script pose timing.

**Thesis:** a strict, owner-authored, one-shot exposure sheet can make frame
holds and substitutions portable and auditable without moving creative or
rendering authority into REEL.

**Deletion target:** one hand-synchronized pose/hold timing table or equivalent
render-script block.

**V1 boundaries:** exact inclusive frame spans; explicit working frame rate;
complete or sparse named tracks; optional asset hashes and narration-cue
bindings; deterministic path-free report. No drawing generation, pose or mouth
selection, interpolation, rendering, DCC mutation, lip-sync automation,
approval, or release.

**Disproof:** reject the slice if frame conversion changes a shot by more than
half a frame, overlaps or complete-track gaps pass, portable evidence leaks
paths, or REEL supplies a creative substitution.

## Audit and comparison

| Analogue | Classification | Finding |
|---|---|---|
| REEL choreography v0.2.24/v0.2.26 | Reuse | Exact frames, 1–120 fps, SHA-pinned production binding, and half-frame shot alignment are established seams. |
| REEL sprite keyframes and intentional holds | Adapt | Renderer inputs already use inclusive frame identity, but choreography currently hard-codes reaction holds and does not expose a general owner-authored X-sheet. |
| KARTS stride-cycle v0.1 | Adapt | Four-frame holds and protected pose ranges are useful owner evidence but live separately from choreography and rendering. |
| BERTICA S1E01 shot 036 | Reuse | The owner has a 19,596 ms cel+sprites shot, an exact anchor cel, moth-effect intent, and three narration cues. |
| Toon Boom Harmony Xsheet | Adapt | Harmony treats the Xsheet as the digital exposure sheet for named drawings and exposure length, alongside a timeline view. REEL needs the portable validation contract, not the editing UI. |
| Toon Boom Storyboard Pro panel timing | Adapt | Storyboard Pro exposes exact frame duration and one-frame panel adjustments. REEL keeps that frame precision but binds one animation shot rather than becoming an editorial UI. |
| Automatic lip sync or DCC project editing | Avoid | These would choose or mutate creative material and exceed the private-preview proof. |

The BERTICA duration is not an integer number of frames at common rates.
Therefore V1 calls `fps` a working timebase, uses exact integer comparison
against REEL's canonical whole-millisecond production binding, and reports
rather than hides the alignment error. It does not claim sub-millisecond source
precision.

## Five-role findings

| Role | Finding | V1 response |
|---|---|---|
| Animation Director | Holds and substitutions must be explicit, but planned art must remain representable before final hashes exist. | Inclusive spans, optional declared asset hashes, explicit planned counts, and no claim that REEL verified asset bytes; no fallback selection. |
| Editor | Frame rounding must not silently alter shot duration. | Exact integer comparison against the bound shot; reject beyond half a frame and report the residual. |
| Sound Designer | Dialogue may inform exposure without REEL generating mouth shapes. | Optional cue IDs must exist and belong to the same shot; no phoneme or lip-sync automation. |
| Story Director | Exposure evidence must not revise canon or imply creative approval. | Owner-defined opaque exposure IDs; technical report explicitly grants no selection or approval. |
| Platform and Audience | Working animation timing must not masquerade as a delivery-rate decision. | Report states `delivery_frame_rate_claimed: false`; delivery export remains owner-controlled. |

## Slice

1. Validate BERTICA `s1e01-shot-036` as a 490-frame, 25 fps working sheet:
   exact anchor-cel hold plus owner-declared moth-effect and cue relationships.
2. Translate one existing KARTS stride-cycle performer into exact four-frame
   substitutions and protected shot/celebration ranges.
3. Require one accepted report and structured failures for overlap, gap,
   duration drift, stale manifest hash, wrong-shot cue, and overwrite.
4. Delete the duplicated KARTS pose-timing block only after the consumer render
   can consume the owner sheet; deletion is not part of V1 validation.

## Real owner proof

### BERTICA S1E01 shot 036

- Source shot: 19,596 ms, `cel+sprites`, slow push, exact selected anchor-cel
  bytes, owner-declared moth-wing intent, and narration cues 086–088.
- Working sheet: 490 frames at 25 fps over three complete tracks.
- Exact alignment error: 100 milli-frames, or 4 ms at 25 fps; accepted inside
  the 500 milli-frame limit without claiming a delivery frame rate.
- Evidence: one declared anchor-cel hash, two planned exposures, nine verified
  same-shot cue relationships; REEL did not read or verify asset bytes.
- Path-free report SHA-256:
  `da4c1c2991e35d2b28c0aab079aadbab96ae7d97c1ab19cd747aac789ecc5727`.

### KARTS Sheary stride cycle

- Source owner timing: 360 frames at 24 fps, three-phase four-frame holds,
  frames 248–251 protected as `shoot`, and frames 315–359 protected as
  `celebrate`.
- Working sheet: one complete pose track, 80 exact non-overlapping exposures,
  360 covered frames, zero gaps, and zero duration error.
- All 80 exposures remain planned identifiers because this proof validates the
  owner timing translation, not sprite bytes or creative approval.
- Path-free report SHA-256:
  `df0c504216d4353dc6e6dfe064528ecf8388fb78819b847fe4483ee8e8484c70`.

The validation slice proves the shared contract. It does not yet delete the
KARTS stride-cycle input because the current renderer does not consume exposure
sheets; that migration remains the named deletion gate.

## Independent review closeout

- Accepted: cue validation initially ignored REEL's existing
  `shot.narration_cue_ids` fallback. V1 now mirrors production allocation and
  tests direct, fallback, wrong-shot, and unknown cue relationships.
- Accepted: sheet and manifest hashes initially came from separate filesystem
  reads. Both now hash the exact byte buffers that were parsed and validated.
- Accepted: unconstrained production work and shot strings could violate the
  path-free report promise. Exposure reports now require portable IDs before
  serializing either value.
- Rejected as outside the shared contract: deriving half-frame error from
  sub-millisecond source seconds. Production binding canonically resolves whole
  milliseconds; V1 now states that precision boundary explicitly.
- GPT, Claude, and Gemini model-family reviews returned no remaining findings
  after the fixes. Built-in `codex review --uncommitted` was attempted four
  times without changing its model and remained blocked by account usage
  limits.
