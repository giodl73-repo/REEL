# Native motion and comparison recovery

First bounded engine port from PR #89: native identified FFmpeg cadence,
matched still/directed/reduced comparison packages, focal-anchor/pan cameras
and explicit picture viewports. Source commits: `0dbdfa4`, `0c91999`,
`1aa9022`, `8bd6bdf`. Current main's later presentation and asset functionality
is retained; this is not a replacement with the old branch's file snapshots.

The native regression matrix covers frozen moving intervals, portable matched
comparisons, focal/pan pixels and audio, phased holds/reduced motion, reserved
caption geometry and landscape/portrait viewports. These checks run explicitly
on Windows and Linux in CI. New optional fields preserve default serialization.

Producer-applied animation/editor/sound/provenance review: geometry and cadence
are measurable engine properties; readable, appropriate motion and selected
episode acceptance still require project review. No private consumer evidence,
creative selection or complete-episode qualification is asserted here.

Layer validation, temporal composition and native audio/delivery hardening
from the old PR are separate subsequent ports. Historical production notes
remain on its original branch and archival tag; they are not completion claims.
