# Sprite choreography staging (v0.2.33)

REEL can bind a portable sprite materialization receipt to an existing
choreography asset map without serializing a workstation cache root into the
customer project.

`reel.sprite-choreography-binding.v0.1` pins the choreography, original asset
map, and materialization receipt hashes. Each mapped performer identifies a
materialized character request for its default image and for every named pose.
Every remaining performer must be explicitly listed under
`preserve_unmapped_performers`; silent partial migration is rejected.

```text
reel sprite-choreography-stage \
  <binding> <receipt> <base-assets> <cache-root> <output-assets>
```

The output asset map is intentionally machine-local. It resolves cache entries,
preserved performers, backgrounds, and props to physical paths so the existing
choreography compiler and renderer can consume them. It belongs in working
storage, not in a portable repository.

Before writing, staging verifies:

- every portable input hash;
- exact performer coverage;
- exact pose-name coverage against the base asset map;
- character/request existence in the materialization receipt; and
- every physical cache file against its recorded SHA-256.

The printed staging report is path-free and identifies cache bindings through
logical keys. The staged map can then be passed unchanged to
`choreography-sprite-manifest`, followed by the normal v0.2 validation, planning,
rendering, and artifact-verification pipeline.
