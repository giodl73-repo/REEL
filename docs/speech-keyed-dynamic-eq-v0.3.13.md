# Speech-keyed dynamic EQ rendering v0.3.13

REEL v0.3.13 executes the optional `dynamic_eq` block already introduced by
the v0.3.12 audio-ducking schema. It does not infer a frequency, target role,
detector role, or creative balance.

For each ordered policy, REEL mixes only the declared target roles and detector
roles. When `dynamic_eq` is present, the target bus is split into an unchanged
program path and a `bandpass` presence-band path at `frequency_hz` and `q`.
`sidechaincompress` keys that band from the detector bus using the policy
threshold and ratio plus the dynamic-EQ attack and release. A dry-floor blend
caps band attenuation at `max_cut_db`. REEL subtracts only the attenuated band
difference from the target program, then applies the policy's independently
bounded broadband ducking.

The stable order is:

```text
role buses
  -> speech-keyed target-band attenuation
  -> target-specific broadband ducking
  -> bus sum
  -> mastering and exact runtime conform
```

Detector audio is control signal only. Dialogue, narration, ambience, and
effects are not filtered unless their roles are explicitly named as targets.
The D/M/E stems therefore retain the same routing contract and recombination
proof as v0.3.12.

The implementation uses FFmpeg `bandpass`, `asplit`, `sidechaincompress`,
`volume`, and `amix` filters rather than a build-specific dynamic-EQ plugin.
The ignored synthetic stem test executed in both Windows and Linux CI now
enables `dynamic_eq`, renders all outputs, and verifies exact sample geometry
and D+M+E recombination. Unit evidence also binds the filter order and confirms
that broadband ducking still runs after the presence carve.

Successful rendering proves execution of declared policy. It does not prove
dialogue intelligibility, musical suitability, creative approval, or selection
of a mix.
