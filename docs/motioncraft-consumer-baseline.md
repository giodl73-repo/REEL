> Current status: actual CAIMITOS adoption, corrected r4 package, canonical-cache authority and clean reproduction pass. See motioncraft-acceptance-audit.md. Earlier checkpoints below are historical.

# Actual CAIMITOS adapter baseline

The existing sanitized CAIMITOS adapter was executed read-only from its
authoritative checkout, with all generated input/output files in REEL's owned
worktree. No CAIMITOS source, registry, project packet, cache or private media
was changed. This verifies the legacy baseline before a consumer motion
upgrade; it does not implement or certify that upgrade.

CAIMITOS checkout HEAD observed:
`7093354f73fe7a29644d0d0e2b56ba25dc1d9178`.
Exact adapter bytes:
`5f8ce9ef51fba7b43948dfe405118d0ff87fbf43fa84548245eba0853909b7df`.
Exact sanitized source bytes:
`ccbdb3c54fceeec5fe9b906f07f92cdcdf1b5edfc813187b0e6662277abe22aa`.
The hashes identify the bytes actually executed/read; the checkout HEAD alone
does not assert a clean consumer checkout or approval.

## Executed commands

From the Motioncraft REEL worktree, using binaries built from this branch:

```powershell
py C:/src/CAIMITOS/tools/video/reel_scene_delivery_adapter.py C:/src/CAIMITOS/tests/fixtures/reel-scene-delivery/caimitos-sanitized-v1/source.json --output target/motioncraft-caimitos-baseline-r1
reel scene-delivery-render target/motioncraft-caimitos-baseline-r1/job.json --asset-root target/motioncraft-caimitos-baseline-r1 --output-dir target/motioncraft-caimitos-baseline-r1/render
reel scene-delivery-check target/motioncraft-caimitos-baseline-r1/job.json --asset-root target/motioncraft-caimitos-baseline-r1 --output-dir target/motioncraft-caimitos-baseline-r1/render
py tools/motioncraft_canary_check.py target/motioncraft-caimitos-baseline-r1 --source C:/src/CAIMITOS/tests/fixtures/reel-scene-delivery/caimitos-sanitized-v1/source.json --adapter-script C:/src/CAIMITOS/tools/video/reel_scene_delivery_adapter.py --output target/motioncraft-caimitos-baseline-r1/native-check.json
```

The render/check commands emit JSON on stdout without an `--output` option.
The successful check stdout was retained as `check-command-report.json` for
comparison with the render receipt. An initial invocation using that
unsupported option failed before rendering; its empty redirected output is
not success evidence.

## Verified evidence

`native-check.json` independently reads decoded 24-bit stereo PCM rather than
inferring effect position from frame timestamps. It verifies:

- native cues retain 48,001 and 47,999 samples at 48 kHz;
- total duration is exactly 96,000 samples;
- `picture-1` and `picture-2` retain partitions 0–24 and 24–48;
- the visual boundary at frame 24 has a native residual of 1/2000 frame;
- the effect occupies samples 123 through 138, with zeros elsewhere;
- D and M remain silent, and the mix equals the original E pulse;
- FFprobe independently decodes 48 picture frames at 24 fps;
- the existing job contains no motion direction and keeps its picture IDs.

The job hash is
`4073e4c395863030bcff40d9aed86637445438f27cdb2ca4f3fc31149ff2eaca`.
The render receipt hash is
`40cbd64fb7644675342dbc8eec513ebf4597af646deec9b43930723fa84112b7`.
These are synthetic local engineering artifacts, not narration performances.

## Remaining consumer gate

Re-running `Enter-BerticaProject.ps1 -Project animation-vfx -AsJson` still
reports `requires-dedicated-worktree`, with Asset Authority required to
provision or clear it before production. The session-handoff skill and
collaboration protocol prevent writing the consumer adapter through the
shared admin checkout or a protected live lane. The earlier request for a
designated Motioncraft integration lane remains unanswered.

The required lane should own the adapter changes, consumer motion schemas,
new/existing episode examples, synthetic motion canary tests, adoption
documentation and session packet. Actual adapter-to-authoring compilation,
explicit versioned upgrade/rollback, illustrated hold/push and canonical
CAIMITOS cache ingestion/hydration remain unmet. A REEL-only overlay applied
to this legacy canary would not close those requirements.
