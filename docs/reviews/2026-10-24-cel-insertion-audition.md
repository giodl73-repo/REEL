# Proposed cel audition review

Target: browser insertion preview, using existing source clocks/media and native
proposal payloads. Method: producer-applied editor, animation-direction and
rights/provenance checklists; no independence or human approval claimed.

- Editing — advisory; owner: tooling producer. Both holds remain exact integers
  inside the source hold. Approximate browser playback is explicitly separate
  from native clocks. Stop/edit/navigation/close invalidate pending playback;
  end-of-cue delivery cannot advance into another recording. DOM checks cover
  boundary event ordering, mixed cue rates and loading cancellation.
- Animation direction — advisory; owner: tooling producer. The chosen picture can
  be inspected before download; audition switches the proposal preview without
  inserting a row, replacing a picture revision or changing the source sequence.
  Controls-only sessions keep timing without media, and ordinary movie sessions
  do not gain primary-recording audition.
- Rights/provenance — advisory; owner: tooling producer. Existing explicit media
  exports and primary recordings are reused. No new source binding, media copy,
  cache operation, write endpoint, render, selection or approval is added. Actual
  listening and visual acceptance remain with the project authorities.

Verification: 29 JavaScript/DOM tests, existing actual JavaScript-to-Rust proposal
integration and matching-host tests. Bounded synthetic browser evidence exercises
real image decode/playback at two native rates; producer records retain raw
browser evidence and runtime hashes. No private media belongs in REEL.

No critical/major finding remains in this bounded change. Richer score/Sonic/VFX
intent, production use and human acceptance remain separate work.
