# Shot-scoped effect preview contract (v0.3.20)

REEL `0.3.20` can render one exact shot from a larger preview-ready production
manifest without encoding the rest of the timeline. Selection is explicit and
fails closed when `--shot-id` is missing or ambiguous.

```text
reel animatic-render production.yaml \
  --asset-root assets \
  --silent --no-captions \
  --shot-id shot-014 \
  --output shot-014-effect.mp4 \
  --clean-output shot-014-clean.mp4
```

The effect output retains the shot's ordered, hash-pinned effect passes. The
optional clean output removes those passes while retaining the exact base
picture, duration, camera treatment, canvas, frame rate, and encoding profile.
Full-manifest captions and A/B audio variants are intentionally rejected in
this bounded preview mode.

Each artifact report keeps the original manifest as a hashed input and adds
`render_scope` lineage with the exact shot ID, effect/clean variant, original
timeline start, conformed duration, and frame count. `animatic-check` rebuilds
that scope from the original manifest before validating the render. Run
`effect-pass-check` on the effect artifact; the clean artifact is expected to
pass `animatic-check` and to contain no complete effect pass.

The contract is provider-neutral and accepts no project-specific effect
semantics. Synthetic fixtures cover exact selection, missing and ambiguous ID
rejection, clean/effect graph separation, shared base-picture lineage, exact
duration/frame count, ordinary artifact verification, and effect-pass
verification.
