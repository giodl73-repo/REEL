# Chapter score fixture

This synthetic fixture proves manifest-owned, provider-neutral music direction.
It contains no audio or visual binaries. Validate and compile it with:

```powershell
cargo run -- validate manifests/fixtures/chapter-score/manifest.yaml --output json
cargo run -- score-plan manifests/fixtures/chapter-score/manifest.yaml --output json
```

`score-plan` does not synthesize or license music. It creates a strict creative
handoff that a human composer, library search, music model, or renderer adapter
can consume without translating an unstructured paragraph.
