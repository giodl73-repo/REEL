# Layered sprite libraries (v0.2.31)

REEL can now validate and resolve portable layered-sprite contracts without
embedding sport, character, or production knowledge in the renderer.

The contract separates four artifacts:

1. `reel.sprite-library.v0.1` owns generic poses, anchors, ordered layer slots,
   transform stages, and a logical cache namespace.
2. `reel.sprite-profile.v0.1` maps a domain's opaque selectors to exact generic
   poses. A declared fallback must identify its reason; undeclared nearest-pose
   guessing is rejected.
3. `reel.sprite-cast.v0.1` binds shared skins and character-specific identity
   layers to stable subject IDs and named pose requests.
4. `reel.sprite-cache-plan.v0.1` emits content-derived logical cache keys and
   ordered composition recipes. It contains no machine-specific cache path.

Pose-owned body and equipment layers are transformed first. Skin-owned uniform
and identity layers follow the pose. Readable decals, such as numbers or text,
can be marked `post-transform` so mirroring a left-facing body never reverses
the lettering.

Character layers override shared skin layers. This lets a production reuse one
uniform recipe while binding a distinct face treatment and readable number for
each performer.

## Commands

```text
reel sprite-library-validate <library>
reel sprite-profile-validate <library> <profile>
reel sprite-cast-resolve <library> <profile> <cast> [--output-path <plan.json>]
```

Every dependency is SHA-256 pinned. Cache-plan writing is atomic and refuses to
overwrite an existing file. The fixture in `manifests/fixtures/sprite-library/`
is sanitized and demonstrates mirrored action poses, a front-facing keeper,
per-character layers, and post-transform decals.

This release deliberately stops at deterministic resolution. A later renderer
may materialize the plan into images or an animation manifest, but it must not
invent unresolved domain semantics or silently substitute poses.
