# Timed primary still overlays

Entry `03a21cef64d6b277adeb7fb2e86154eb039ecd36`; native REEL production
adapter scope for Tig WP-010/011, REQ-003/010 and the existing delivery/context
contracts. Add `timed-still-overlay` to the existing external-layer render modes.
No cache, lifecycle, ownership or source-selection protocol is added.

An admitted primary PNG supplies one full-canvas alpha image, held only over
the existing semantic effect/overlay attachment's half-open delivered frame
interval. Require exact output geometry, an alpha pixel format, PNG codec and
exactly one decoded frame. Reject fonts, alternate render sources and derivation
receipts: this mode consumes the evidence object directly. Reject ordinary RGB
stills and animated/multi-frame input. Existing timed video semantics, rate and
source-frame sufficiency checks remain unchanged.

Animation admission inspects PNG content with the existing decoder, capped at
8 MiB of decoder allocation and the declared canvas dimensions. Do not rely on
demuxer frame counts alone: a Windows extended path can expose an APNG's default
frame as an ordinary PNG. Media probing and guarded source reads remain required.

Reuse existing sample/frame conformance, source guards, lossless layer order,
output contracts, receipts, rendering and independent primary-reference replay.
Loop the selected PNG explicitly at the delivery frame rate; bound the composed
output to its admitted frame count. Stage the exact source as PNG, account for
it in the output set and verify its pin. Do not create or enroll an intermediate
movie as a production dependency. Existing source/context/resource admission
and caller process containment remain required.

Entry: selected source metadata and exact guarded input pins. Exit: existing
native checked-candidate/receipt interfaces; production, creative approval,
retention and publication remain separate. L0: fmt, diff and warnings-denied
Clippy. L1/L2: synthetic alpha PNG crossing a picture cut, exact span and source
identity, outside-span preservation, output planning, primary-reference replay,
invalid geometry/alpha/source fields and multiple-layer ordering; existing video
overlay and delivery tests remain regression controls. Actual private project
conversion and installed runtime adoption remain separate.
