# Explicit static picture fitting

Entry `d440986ca0c13880ad72b2a15db2edd9c2db4e7f`; native REEL delivery
scope, Tig WP-010/011 and REQ-003/010 producer/context contracts. A picture may
declare `fit: "contain"` or `fit: "stretch"`. Omission preserves the existing
contain-and-pad filter and is omitted from serialized jobs. Stretch explicitly
scales the cropped source into the declared picture region without aspect-ratio
preservation; it cannot be combined with a motion recipe. All source guards,
canvas/region admission, output contracts and primary-reference checks remain.

This supports retained recipes that deliberately resize source cels to a slightly
different aspect ratio. Input dimensions alone never select stretch implicitly.
No source image, delivery selection, approval or installed runtime changes.

L0 fmt/diff/Clippy; L1/L2 synthetic wide white image: default and explicit contain
have identical decoded frames and padding, explicit stretch fills the canvas,
primary-reference replay agrees, and stretch plus motion refuses. Existing
delivery regression and explicit legacy sample/frame checks remain controls.

Local checks pass: 31 regular delivery cases (15 explicitly ignored native
cases), the separate native sample-offset/stem/frame/tamper case, all-target/
all-feature locked Clippy with warnings denied, formatting and whitespace.
Rust callers constructing `Picture` literals supply `fit: None` to retain the
default; older JSON needs no new field. Hosted and project adoption remain open.
