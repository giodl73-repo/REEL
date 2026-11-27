# Music production conformance

The `reel-music` crate can compare a production policy with validated recording
observations. REEL resolves the analysis back to the exact source contract
before checking it. Label and artist are supplied by the caller, along with a
digest of its intent materials. The caller resolves label defaults, artist
requirements, track briefs and explicit exceptions into the policy.

```powershell
cargo run --locked -p reel-music --bin reel-music-conformance -- policy.json analysis.json
```

A policy uses schema `reel.music-conformance-policy.v1`. Give it a track ID,
label, artist, source contract SHA-256, intent SHA-256, and the standards that
apply to the track.

- **Duration** compares decoded sample count and sample rate with a target and
  tolerance in milliseconds.
- **Tempo** compares eligible timestamped observations with a target and
  tolerance in milli-BPM. A policy can allow half- or double-pulse
  equivalence. Mixed or out-of-range eligible candidates are reported as
  conflicting evidence. A rubato scene can leave the tempo standard unset.
- **Instrumentation** compares exact normalized labels and explicit aliases.
  A track-wide artist commitment and a track-specific request both ask for
  positive source evidence. Ensemble palette entries are context only. An
  input with no eligible matching observation stays unverified.

Tempo and instrumentation policies select analyzer fingerprints and minimum
confidence. `conformance::analyzer_fingerprint` derives the pin from the
adapter, version, model revision and parameter hash. Build that list from
calibration evidence. For saxophone, include examples with and without sax
and test confusion with clarinet and trumpet. Isolated and masked passages
exercise different parts of the intended detection task. An eligible positive
observation establishes reported presence over its sample range. A separate
musical review is needed to show a recognizable theme or a featured solo.

The report preserves the policy and analysis hashes and the accepted
observation IDs and source sample ranges. It produces production findings.
Artistic ratings for groove, phrasing, development and payoff belong to the
listening rubric. The CLI runs offline and consumes existing REEL evidence;
it does not execute a transcription or instrument-recognition model.
