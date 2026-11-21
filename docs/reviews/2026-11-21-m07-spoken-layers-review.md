# M07 spoken music and Sonic layers: checklist review

**Status:** local qualification passed on merged M06 `cef69f7`;
required hosted qualification pending.
One assistant applied the required Sound, Editor and Rights checklists, plus
Music Reconstruction for signal identity and Platform/Audience for scope.
This is not independent review, listening acceptance or human approval.

## Findings and disposition

| Lens / finding | Disposition |
| --- | --- |
| Sound / semantic events must use their consuming edition's clock | Resolve source-pinned markers from a fresh original compilation during preparation. The four effect starts are 160800, 369600, 194400 and 432000 samples, independently specified before execution. Unselected/stale/sub-ms markers refuse. |
| Editor / an authored duck window is different from automatic dialogue detection | Existing hold automation applies an explicit two-anchor reduction. No adaptive compressor, limiter, normalization or time stretch is inferred. Fades, complete tails and overlap permissions are declared; actual masking and pacing remain listening gates. |
| Music Reconstruction / a floating timestamp can put a correct marker one block late | The first full-stream comparison caught a 48-sample late duck. Explicit float format, 48-sample framing and integer 1/1000 timebase correct the representation. The original two-unit M/E and four-unit mix tolerances were preserved. D remains bit-exact and silence outside support remains exact zero. |
| Rights / nearby catalog files cannot substitute for an explicit provider refusal | The existing Sonic resolver gains an explicit-path callback while retaining all authority, full-byte and measured-geometry checks. Its old filesystem wrapper remains intact. A separate test proves provider failure is terminal even when a valid catalog-relative file exists. |
| Rights + Sound / parent speech evidence must stay distinct from a mixed candidate | Preparation independently checks the M04 parent once and seals that witness. Produce/check reread the actually consumed D/M/E bytes and pinned Sonic controls. D remains the original checked waveform; a new bed changes the mixed identity and output without replacing D or its parent recipe. |
| Platform/Audience / runtime and clock support must be specific | Explicit executable enrollment reuses `BoundTool` protection on Windows. Other platforms retain cooperative rechecks and do not claim native protection. The local FFmpeg build rejects the old script option, so the bounded graph uses direct argument transport. No renderer or process engine was duplicated. |
| Rights + recovery / a partially written package must not appear complete | The new destination and private staging are operation-owned. Only fixed no-clobber WAV names are published; receipt is last. Timeout, interruption, existing destination and foreign-file cases refuse without selection or release. Concurrent destination writers remain unsupported. |
| Platform/Audience / the cooperative runtime read must retain the adapter's tighter cap | The shared observer now has a bounded internal entry; its existing default ceiling stays unchanged. The cooperative layer path uses the exact declared executable length. A focused test rejects a larger file, an enlarged file and an oversized requested bound. |

No music is generated, reconstructed or arranged. Score/arrangement and lyric
adaptation add no new authored layer here. Story/Animation also add no pictures
or narrative claim. M06's real speech, language and listening checkpoint remains
open; technical qualification cannot promote those decisions.

## Requirement-to-test map

| Requirement | Exact case |
| --- | --- |
| Four language/audience clocks, full-stream independent D/M/E/mix oracle, duck/fade boundaries and original D | `spoken_programme_media`: `layers::real_four_edition_layers_follow_native_clocks_and_independent_pcm` |
| Bed v2 changes actual mix bytes and mix identity, preserves dialogue bytes/recipe, and rejects original controls or rehashed false PCM | `layers::revised_bed_changes_mix_without_changing_dialogue_and_tamper_refuses` |
| Stale source/marker/language, missing authority scope, undeclared bus/dialogue overlap, invalid gains/fades, zero deadline, injected interruption and existing/foreign output | `layers::stale_scope_overlap_gain_tail_and_clock_refuse_before_writes` |
| Finer-than-ms anchor rejects without rounding; full effect tail cannot exceed the edition | `layers::finer_anchor_and_full_effect_tail_overrun_refuse_without_rounding` |
| New Sonic input seam preserves exact byte/geometry checks and has no filesystem fallback | `sonic_assets_v0318`: `explicit_consumer_paths_verify_bytes_and_never_fall_back_to_catalog_paths`; existing Sonic cases cover prior authority/resolution behavior |
| Parent contract, independent raw-speech checks, native retention and review records | Existing `spoken_programme`, `spoken_programme_media`, `spoken_programme_history`, `cue_relative`, `audio_event_clocks` and `v0213_cli` regressions |
| Exact cooperative runtime observation cap | `media_runtime::tests::bounded_observation_keeps_declared_byte_cap` |

The four external-runtime cases are ignored in ordinary unit runs and explicitly
executed in **both** hosted media lanes. No missing-runtime successful skip is
allowed. Test fixtures create only public signals and temporary controls;
there is no production cache, provider, private voice, upload or project adoption.

## Execution

Exclusive Cargo target: `F:/tmp/reel-builds/media-m07-isolated`. Windows,
Rust 1.98.0; dev/test debug symbols disabled. The initial complete four-case
real-media run passed in 54.09 seconds, none ignored in the explicit invocation.
Individual artifact fixtures retained their measured 60-second outer budget.
Executable enrollment precedes artifact-fixture timing; it separately checks a
bounded explicit pin and retains the native guard. This is not a startup-speed
benchmark or operating-system I/O preemption claim.

Local FFmpeg: `N-126390-g9fc8c785e2-20260902`, 145,872,384 bytes, SHA-256
`21e273a2952340f73abc1572c479af54b400b5949f3a9c0102b22eee6d35df29`.
Hosted lanes must qualify their own pinned 8.1 runtime; local success is not
cross-runtime byte-identity evidence.

Final merged-parent regression command passed 64 cases: 24 existing media,
20 declaration/compiler, nine clock, four native history, six Sonic and one
existing review CLI case. Four new runtime cases were listed as ignored there
and then explicitly executed: all four passed in 56.76 seconds. After tightening
the cooperative observation cap, its new unit case passed (0.02 seconds), the
four runtime cases passed again (76.38 seconds), and all-target/all-feature
Clippy with warnings denied, formatting and diff checks passed again. This is
**69 distinct passing tests**. The last four-case wall time includes shared tool
enrollment before artifact-fixture timing; every measured artifact fixture kept
its original 60-second bound. The legacy Sonic real-render test remains a local
ordinary-run ignore and is separately required in the existing Linux CI lane.

Commands and exact numeric limits are in the
[layer contract](../spoken-edition-layers-v1.md); the additional cap check was
`cargo test --locked --lib media_runtime::tests::bounded_observation_keeps_declared_byte_cap -j2 -- --exact`.
Local documentation links and actionlint 1.7.12 passed.
Required hosted qualification passed on exact head `6ee8262` in
[CI 44933453094](https://github.com/giodl73-repo/REEL/actions/runs/44933453094),
21 minutes 7 seconds total. Both hosted media lanes executed the four new real
runtime cases. [PR #387](https://github.com/giodl73-repo/REEL/pull/387) merged as
`1db7679`; the merged tree matches the qualified head exactly. This closes M07's
engineering gate without closing the separate decisions below.

Listening for intelligibility and naturalness, actual voice/text/music rights,
creative selection, publication, native retained mix packaging and producing
project adoption are separate and unperformed. The detailed receipt is private;
the check summary contains fixed technical/authority flags only.
