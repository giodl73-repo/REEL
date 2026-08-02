# Pulse 01: Format and series bible

## Outcome

Opened REEL's first episodic-series work and defined its complete Season One
story architecture before screenplay or production work.

## Changes

- Added `formats/episodic-series.md`.
- Added `works/0003-reading-the-runes/brief.md`.
- Added `works/0003-reading-the-runes/series-bible.md`.
- Added the wave and first-pulse records.
- Preserved BANISH as the source owner for catastrophe and causal-inference canon.

## Validation

```powershell
git grep -n "REEL" -- README.md PRODUCT_PLAN.md context\waves\PHASES.md
git diff --check
cargo test --quiet
```

## Next pulse

Run the series bible through the story-director, editor, sound, animation, and
platform/audience roles before expanding the Atlantis treatment into a scene
beat sheet.

