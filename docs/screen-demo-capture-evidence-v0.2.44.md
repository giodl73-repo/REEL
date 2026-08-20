# Screen demo capture evidence (v0.2.44)

REEL v0.2.44 records immutable, path-free evidence over owner-created CLI,
TUI, and Web PNG captures that are intended to demonstrate one exact product
state.

REEL does not execute owner commands, launch a TUI, control a browser, create
captures, inspect visible semantics, verify redaction or accessibility, select
footage, or approve publication or release.

## Input

```json
{
  "schema": "reel.screen-demo-capture-input.v0.1",
  "demo_id": "owner-product-demo",
  "owner_state_ref_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "state_document": {
    "file_id": "sealed-card",
    "path": "card.json"
  },
  "required_surfaces": ["cli", "tui", "web"],
  "captures": [
    {
      "capture_id": "cli-card",
      "sequence": 0,
      "surface": "cli",
      "viewport_id": "terminal-120x34",
      "path": "cli.png",
      "width": 1200,
      "height": 680
    }
  ]
}
```

The input is local and path-rich. Relative state and capture paths resolve
against the input file. The owner state reference is a sanitized identity
supplied by the owner system; REEL independently measures the complete state
document but does not infer or validate an embedded domain fingerprint.

`required_surfaces` must be strictly sorted and unique. Captures must use only
required surfaces and contiguous sequence numbers from zero. Every required
surface must appear at least once.

V1 accepts exact PNG captures only. REEL reads each file once, verifies PNG
decoding and declared dimensions, rejects physical aliases and duplicate
capture bytes, and requires captures to be physically distinct from the state
document.

## Receipt

```powershell
cargo run --bin reel -- screen-demo-capture-receipt `
  capture-input.json --output-path capture-receipt.json --output json
```

The no-clobber receipt contains:

- the exact input and state-document SHA-256 values;
- sanitized demo, state, capture, viewport, and surface identities;
- ordered capture SHA-256 values, byte counts, dimensions, and media types;
- exact required-surface coverage;
- explicit execution, capture, semantic, privacy, accessibility, selection,
  publication, and release boundaries.

It contains no local paths, commands, URLs, source content, terminal text,
browser state, credentials, or raw screenshot pixels.

## Trust boundary

`capture_bytes_verified: true` means REEL decoded and measured the current PNG
bytes. It does not mean the image depicts the named state, that CLI/TUI/Web are
semantically equivalent, that private material was redacted, that the surface
is accessible, or that the capture is visually good. Those remain owner and
human-review responsibilities.
