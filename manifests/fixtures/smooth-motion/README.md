# Smooth-motion cadence fixture

This synthetic grid reproduces slow audiobook motion without BERTICA text,
photos, voices, or private renders. The manifest declares a 20-second,
1280x720, 24-fps rightward pan over the same approximate 3.5 percent travel
that exposed integer stepping in production.

Render smooth and legacy derivatives:

```text
reel animatic-render manifests/fixtures/smooth-motion/manifest.yaml --asset-root manifests/fixtures/smooth-motion --silent --captions manifests/fixtures/smooth-motion/captions.srt --output target/smooth-motion.mp4 --motion-quality smooth --motion-curve ease-in-out --format json
reel animatic-render manifests/fixtures/smooth-motion/manifest.yaml --asset-root manifests/fixtures/smooth-motion --silent --captions manifests/fixtures/smooth-motion/captions.srt --output target/legacy-motion.mp4 --motion-quality legacy --format json
reel motion-analyze target/smooth-motion.mp4 --output json
reel motion-analyze target/legacy-motion.mp4 --output json
```

The published metric is the fraction of adjacent decoded-frame differences
whose average absolute luma is below 0.001. Smooth moving shots pass at or below
10 percent; the legacy fixture is expected to fail.
