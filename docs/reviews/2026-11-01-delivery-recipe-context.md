# Delivery recipe/context review

Entry `9745d6617dd7d215fd6f64d72a62e3ff8086ce9a`. The delivery-template
adapter now derives exact consumed roots, graph obligations and recipes using
existing Tig Source/ProductionContext ports. No new store, claim journal, cache
policy or custom fingerprint algorithm is added.

Producer storage/privacy/editor/sound review: the lowerer remains the authority
for consumed primary references, including nested derivation metadata. The graph
roots include the source container and consumed leaves; its control rule retains
all three control identities. Explicit request/adapter/lowered-control pins bind
the effective aliases even though unrelated alternatives are excluded from the
recipe's consumed fields. Source checkpoint/binding provenance remains separate.
The existing payload selector helper is reused with broader crate visibility;
other scene behavior is unchanged.

Three local metadata cases pass: exact original-context replay with Git/cache
unavailable; unused draft/annotation edits preserve the recipe across different
source checkpoints, while a used replacement or producer change alters it;
changed controls alter the recipe and foreign requests/old contexts refuse.
The scope has exactly three synthetic primary objects and no control payloads
when all controls are authored metadata. These counts prove no physical custody.

An explicit native fixture additionally replays context/recipe before guarded
input preparation and rendering with the original checkout/loose JSON absent.
Its synthetic producer label is external admission, not a runtime attestation or
trusted produced claim. Native delivery checking and PCM comparison remain
distinct from recipe equality. Hosted qualification and managed worker/CLI,
result retention, actual project semantics and production cutover remain open.

Final local validation: ten regular template/context cases pass; the explicit
frozen-context native render/check also passes with exact nonzero music PCM.
All-target/all-feature warnings-denied Clippy passes. Formatting, diff checks
and raw working-file/index byte equality are required before commit. No runtime
installation, live project enrollment or independent review is asserted.
