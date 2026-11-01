# Retained episode contract integration

Current shared REEL rejected retained episode conforms containing explicit tail
holds, master-only check witnesses, repeat-only output clocks and unknown-range
declarations. Those fields now have typed semantics
through registration, planning, execution and receipt generation. Unknown fields
still refuse; no source manifest is rewritten to hide unsupported information.

This adapts the hold implementation from `fa1d382` and master-check witness
implementation from `92891df`/`cda71b5`, clock conversion from `21c3341` and range
interpretation from `c91553c`, preserving current guarded native
providers and producer APIs rather than replacing files with older branch
versions. It does not merge unrelated speech workers or the older phased
checkpoint implementation.

The registrar retains master-proof and validator references as original
production evidence. Parent plans expose nonzero tail holds without granting
execution, payload verification or currency. Rendering checks the full source
prefix, final-picture repeats and zero-audio tail, including displayed RGB;
fractional-clock math is checked and bounded. Explicit tool denials are terminal.

Master-only checking has separate primary, output, evidence and control
providers. A caller-admitted historical validator is hashed, never executed.
Proof issuance requires a successful complete-output check and uses atomic
no-clobber publication. Rechecking still binds current source/plan, exact receipt,
master and tool bytes; it does not authenticate the origin of a supplied
witness. Sealed native proof routes reject legacy witness fields, and derived
current controls remove those fields when replacing the old component. They
preserve the independently declared parent hold.

Producer-applied animation, editor and sound-design review requires intact cuts,
moving picture/audio prefixes, bounded additional duration and strictly silent
padding. Repeat-only conversion preserves source motion without interpolation.
Rights/provenance review requires preserved evidence identity, explicit provider
admission and separation from creative approval or release. No role supplies
independent review, ownership, consent, production selection or publication.

The repeat-only clock retains every selected source frame in order and preserves
the complete PCM prefix. Checked frame counts and source/output rounding bounds
prevent dropping frames, unbounded padding or trimming audio. Unknown range
interpretation must preserve independently decoded RGB. Metadata intake and
non-executable plans validate the same scalar policies as rendering.

The first hold/proof integration passed 13 focused library checks and 60 regular
integration tests. The final clock/range source passes 14 focused episode library
checks and 30 integration tests: nine ordered-conform, seven registration and
fourteen episode-plan checks. The updated Windows guarded-native parent test
also passes with an explicit output clock: a sparse cel edit and one-frame
parent hold produce 97 verified frames while the obsolete witness files and old
movie are unavailable. Warnings-denied all-target/all-feature Clippy, formatting
and whitespace checks pass. Fifteen other scene native tests remain explicitly ignored
in the regular suite; their coverage is not claimed by this local run.

An actual retained bilingual conform now produces 24 metadata-only component
candidates without removing source fields or opening primary media. Independent
authored-schema and correspondence checks confirm every selected subject, all
authored fields and all 152 declared evidence streams. This is candidate intake,
not payload verification, producer authentication, runtime adoption or delivery
approval. Exact tool/source pins and detailed reports remain private asset-side.

The synthetic regressions cover full-check witness issuance, absent unconsumed
outputs, changed inputs/receipts/masters, opaque provider paths, explicit
validator refusal, metadata bounds, ordered bilingual holds and fresh native
composition. Displayed RGB checks and known-color preservation strengthen the
original hold helper's packed-picture check; matching YUV planes alone cannot
establish unchanged displayed color.
Private production, runtime installation, retained-data adoption and lane
cutover remain separate acceptance steps.
