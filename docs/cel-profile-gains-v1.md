# Native profile gain proposals

The cel tool can propose changes to existing `score_gain_db` and default
`sonic_gain_db` fields in `reel.scene-delivery-profile.v1`. These are scene-wide
delivery controls, not per-cel mix edits. Every language in this source is named
in the proposal and must be explicitly acknowledged, even in a language-scoped
cel session. Other consumers of the same original file keep their original bytes;
this operation writes a new candidate and never retargets an external consumer.

Exports with compatible native gain fields add `profile_controls` metadata. Older
or unavailable profiles keep their existing editor behavior without this panel.
The form starts with no changes enabled and no scope acknowledgment. Choose one
or both gains, acknowledge all listed languages and download the proposal. Dirty
cel edits, comments or general notes must be applied first and the source reopened.
Unchecked fields remain unchanged. An unset Sonic gain inherits the native
default; setting it creates an explicit value. Per-binding Sonic overrides keep
their precedence. Playback remains the unchanged primary recording and pictures;
these controls do not audition a delivered mix.

```json
{
  "schema": "reel.cel-sorter-profile-edit.v1",
  "scene_id": "example",
  "expected_project": "<exact descriptor SHA-256 when mapped>",
  "expected_inputs": {"<each native control role>": "<exact SHA-256>"},
  "affected_languages": ["en", "es"],
  "acknowledge_shared_profile": true,
  "changes": {"score_gain_db": -24, "sonic_gain_db": -30},
  "human_approval": false
}
```

Apply to a new directory:

```text
reel-cel-sorter apply-profile <native-project> <profile-edit.json> <new-candidate>
```

The existing profile and native scene grammars are unchanged. The bounded
operation admits one or two finite gain values between -60 and 60 dB. This is a
tool proposal limit, not a replacement for full native profile validation. Empty,
no-change, null, unknown-field and duplicate-key requests reject. Request size is
limited to 64 KiB. Recursive duplicate keys in the source profile also reject
before normalization. Source/descriptor locks, exact language-set acknowledgment
and `human_approval:false` are required. Stale or incomplete requests fail before
output creation. I/O failure may leave a partial new candidate for explicit
reconciliation, as with the existing native writer.

Only the two requested scalars change. All other profile values, including fades,
dimensions, encoding, sample/frame clocks, dialogue trims, per-binding Sonic
overrides and VFX modes, remain intact. Other control files reuse their exact
captured bytes, including all languages' scene/graphs/bindings and alignments.
Existing output directories refuse. Legacy and mapped single/multi-language
projects use the same operation and source locks.

Register the candidate controls, compile **every** affected language and use the
existing managed render/approval process. Prior receipts stay bound to the old
profile hash. A written candidate is not proof of full profile validity, storage
admission, delivered loudness, rights, or human approval. No media lookup, mixing,
resampling, render, cache promotion, selection or publication occurs here.
