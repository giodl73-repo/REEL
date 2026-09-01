# Series runtime planning and pacing audit v0.2.22

REEL CLI v0.2.22 adds an editorial planning layer to `reel.series.v0.1`.
Existing series manifests remain valid. Exact scene, episode, and season timing
continues to belong to conformed child manifests and the existing composition
contract.

## Runtime plans

Seasons and episodes may declare an optional `runtime_plan`:

```yaml
runtime_plan:
  class: standard
  minimum_seconds: 840
  target_seconds: 1020
  maximum_seconds: 1260
  components_seconds:
    poem: 120
    narrative: 720
    visual-breathing: 120
    titles-and-credits: 60
```

The class is an editorial label such as `short`, `standard`, `long`,
`calibration`, or `finale`; REEL does not impose a vocabulary. Durations must be
positive and satisfy `minimum <= target <= maximum`. When component budgets are
present, their total must equal `target_seconds`. Component names are likewise
project-owned so documentary, audiobook, sports, game, and trailer workflows
can use appropriate terms.

Runtime plans describe intent. Falling outside a range does not invalidate a
series or block an intentionally long finale, protected silence, or unusually
brief episode.

## Timing audit

```powershell
reel series-timing-audit series.yaml --output json
reel series-timing-audit series.yaml --neighbor-drift-percent 25 --output json
```

For each episode, the audit chooses the strongest available runtime basis in
this order:

1. non-zero declared episode runtime;
2. non-zero raw orientation estimate;
3. planned target;
4. unavailable.

The report always labels the selected basis. A planned target contributes to
projected runtime but is reported as `planned`, not as measured compliance.
Declared or orientation timing is classified as `under`, `within`, or `over`
the episode range. Season reports preserve the same distinction and say
`mixed` when their projection combines timing bases.

The report includes:

- planned and unplanned episode counts;
- planned target and projected full-series runtime;
- season projections and derived sums of episode budgets;
- season-plan alignment with the sum of its episode targets;
- per-episode target deltas, narration/pause shares, and named component budgets;
- under-range and over-range episode IDs;
- adjacent-episode duration changes above the selected warning percentage,
  including the timing basis used on each side of the comparison.

Neighbor drift is diagnostic. It identifies rhythm changes that deserve an
editor's attention but does not assume that equal episode lengths are desirable.

## Recommended lifecycle

1. Assign rough runtime classes and ranges while editing the season slate.
2. Use raw orientation estimates before voice approval.
3. Replace estimates with measured narration, protected pauses, scene timing,
   and composed runtime as production matures.
4. Run the audit after each calibration episode and before season picture lock.
5. Keep justified exceptions; revise accidental pacing drift.

This separation preserves creative judgment while making cross-episode pacing
visible and repeatable.
