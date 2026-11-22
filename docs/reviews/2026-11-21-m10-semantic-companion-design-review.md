# M10 semantic companion: source, role and qualification review

Status: implementation under qualification on merged M09 `b95a6c2`, [PR #395](https://github.com/giodl73-repo/REEL/pull/395). Local qualification passed 66 distinct tests and all-target/all-feature Clippy. Required hosted qualification on the current main base remains to be recorded. This is one assistant applying repository `.roles` checklists, not independent review or creative approval.

The [illustrated contract](../semantic-visual-companion-v1.md) describes the language example, supported API, bounds and evidence distinctions.

## Source audit and selected boundary

The independent spoken checker already recompiles the original controls before comparing the produced PCM. `CheckedPackage::compiled_plan()` now exposes that internally recomputed plan through a read-only accessor. No freely constructed compiled edition or self-reported receipt establishes the original subject.

`companion::Inputs` borrows the original `CheckRequest`, source/candidate/visual/access providers and access-manifest bytes. `prepare`, `produce` and `check` recheck those original relationships. The native path validates an existing lowered browser plan and its actual provider bytes; it creates no asset definitions, cache engine, checkpoint or custody claim. Existing host enrollment, capacity and held byte/program guards remain required. Original programme filenames and admitted asset IDs are distinct namespaces: SHA-256/length and actual bytes establish object correspondence, while original references remain unchanged. The browser fixtures intentionally use different SVG/audio asset IDs. A self-consistent different admitted audio object still refuses against original controls.

Phrase bindings reuse `spoken_phrase_sample` against original word evidence and the named phrase marker, then apply the real compiler's selected-take placement. Engineering markers require an engineering signal and explicitly claim no words. These paths are intentionally separate: lexical fixtures have metadata only and no audio files; real PCM/render fixtures keep engineering-signal classification.

The first native profile is a full-frame SVG with two events, one/two editions, 48 kHz stereo PCM24, 24 fps, ten seconds/240 frames and at most 640×360 pixels. Its explicit full-source zero-fade window reuses the native exact-PCM comparison before output publication. No implicit padding, trimming or alpha-overlay claim.

Metadata operations have a 30-second cooperative deadline. One single-edition render has a 90-second cooperative aggregate, with at most 60 seconds for the existing worker. Two language renders are separate operations. Existing worker limits retain 2 GiB/64 browser processes, 256 MiB/four encoder processes, 128 MiB scratch, 16 MiB output and 30-second I/O wait. Cold hosted Edge startup is already accounted for in the existing fixture envelope. These are host/operation bounds, not a new sandbox or memory-custody guarantee.

## Requirement-to-test map

| IDs | Actual cases and independent oracle |
| --- | --- |
| C01 | `lexical_metadata_resolves_independent_english_and_spanish_entrances` checks original phrases, missing/ambiguous phrases and explicit occurrences. `lexical_metadata_uses_real_compiler_take_placements_without_media` uses the real compiler, including a 2000-sample source trim, while its provider refuses media access. Both are lexical metadata only. `bilingual_markers_use_sealed_audio_and_keep_text_access_signal_basis` independently checks actual produced synthetic PCM/access with engineering labels. |
| C02 | `changing_english_take_moves_only_english_companion`: English clock/evidence changes, Spanish remains byte-identical and event IDs stay stable. The native case also renders replacement English and expects frame 25 instead of 24. |
| C03 | `native::real_native_bilingual_svg_transitions_keep_exact_pcm_and_follow_replacement`: independent RGB decode checks every pixel/frame against red/green/blue states; independent PCM decode compares the original fixture samples. |
| C04 | `invalid_bindings_stale_audio_text_and_interrupted_publication_refuse` checks closure, edition/performance/marker identity, evidence-kind confusion, geometry, coincident frames, stale text/audio and deadlines with diagnostic-class assertions. `altered_clock_and_browser_pins_cannot_replace_original_checked_controls` rejects fully rehashed false clock subjects and terminal provider refusal while the original SVG genuinely exists on disk. `nonintegral_duration_and_rehashed_false_candidate_receipt_refuse` checks whole-frame duration, byte bounds and independently detected wrong PCM despite rewritten audio/receipt hashes. |
| C04 | `phrase_binding_cannot_turn_engineering_markers_into_spoken_words` refuses wrong observation kind and changed native word evidence. |
| C05/C07 | Bilingual marker golden preserves the exact authored Spanish/English text separately from checked engineering access labels. It asserts the exact signal label, null spoken text basis, private canary absent from summary and all acoustic/authority/device/release flags false. |
| C06 | Invalid/publication case cancels after directory reservation and leaves no committed marker or owned directory. `native::native_companion_cancellation_prevents_final_output` interrupts after a worker intermediate appears and verifies cleanup. Native/golden cases refuse overwrite and preserve original output bytes. |
| C06 | `earlier_companion_reopens_with_retained_inputs_after_replacement` removes original source directories, re-prepares retained test bytes, reopens v1 after v2 and refuses missing v1 data while v2 still checks. The copied test provider is not a production asset store, migration or custody claim. |

The hand-derived native visual oracle is:

| Edition | Red frames | Green frames | Blue frames | Source sample entrances |
| --- | --- | --- | --- | --- |
| English v1 | [0,24) | [24,48) | [48,72) | 48000 / 96000 |
| Spanish v1 | [0,30) | [30,60) | [60,84) | 60000 / 120000 |
| English replacement | [0,25) | [25,48) | [48,72) | 50000 / 96000 |

These are synthetic signal and SVG fixtures, not recordings of the example phrases. The separate lexical metadata oracle uses the exact English/Spanish phrases in the contract.

## Checklist findings

| Lens | Finding / disposition |
| --- | --- |
| Animation | Existing SVG/frame-event pipeline produces three visible states. Actual decoded pixels, rather than an index or render status, are the oracle. No new compositor or animation grammar. |
| Story | Two authored event IDs preserve intent across independently phrased languages and take replacement; original wording/order stays upstream. |
| Editor | Original sample positions resolve through selected-take placement and one shared rounding operation. Distinct interior frames are required; no silent padding or reordered event intent. |
| Sound | Independently checked audio stays upstream of picture. The explicit zero-fade whole-source window checks exact PCM before publication; picture is unnecessary for audio-only production. |
| Platform/Audience | Verify original transcript/access bytes. Engineering sources remain labeled signals. Typography, reading pace and real-device acceptance are unperformed. |
| Vocal Adaptation | Exact phrases match source/word metadata; engineering markers cannot masquerade as words. No synthetic signal is treated as recorded speech or proof of pronunciation/translation/listening acceptance. |
| Rights/Provenance | Preserve original pins/scopes, held subjects and private identities. Portable counts/flags grant no voice, language, performance or release authority. No real project media/provider/cache was used. |

Unused score/arrangement and music-reconstruction lenses: no score alteration, transcription, note clock, stem repair or musical arrangement occurs in M10. The existing lesson score remains its upstream checked reference.

## Local execution and hosted enrollment

Exclusive Cargo target: `F:/tmp/reel-builds/media-m10-isolated`; Windows/Rust 1.98.0, dev/test debug symbols disabled. All-feature type checking passed.

- Six explicit metadata/runtime cases: **6 passed, 0 failed, 0 ignored**, 167.45 seconds.
- Two explicit native cases: **2 passed, 0 failed, 0 ignored**, 144.93 seconds. Three SVG/video outputs covered English, Spanish and replacement English. Every inherited artifact fixture retained its original 60-second outer budget.
- At this initial checkpoint, lexical unit results, regressions, Clippy and hosted outcomes were pending. The final checkpoints below supersede that status.

CI enrolls all six metadata cases in Linux/Windows core shard 2 and both native cases in Windows core shard 2, with explicit browser/FFmpeg enrollment. Ordinary ignores, absent runtimes and metadata planning alone cannot establish executed native qualification. Existing reading, lesson, native/media and required Rust/FFmpeg checks remain in place.

Actual acoustic/linguistic accuracy, human listening, visual/device acceptance, authority, selection and project adoption remain separate and unperformed. This engineering qualification does not close M06's real spoken-performance listening gate.
## Runtime-hash qualification performance finding

The initial wider reading regression reached its content assertions but three
cases failed their inherited 60-second fixture guard at cleanup. That guard is
unchanged. The unoptimized 46,131,200-byte CLI runtime-identity probe took 2.97s
before its expected missing-alignment refusal; platform SHA-256 of the same file
took 0.50s. No probe output was created.

The dev/test profiles now optimize only the SHA-2 dependency at level 3. No
application optimization, assertion removal, identity caching or test skipping
is introduced. Every runtime/input identity is still read and checked. Repeated
byte verification therefore retains its semantics while avoiding an unnecessary
cryptographic debug-build cost. Final reading/lesson/companion reruns and hosted
results remain required before merge; the earlier failed timing run is not a
passing qualification claim.
## Final local checkpoint at implementation head 87eb0ec

All four lexical/preflight cases passed (0.02s). The final eight-case companion
run passed in 66.66s with zero ignored/failed cases, including admitted IDs that
differ from original filenames and a self-consistent substituted audio object.
The exact frame/PCM native cases ran in that invocation.

Existing reading qualification passed all five cases in 120.86s; lesson
qualification passed all five in 63.48s. The previously timing-failing layered
reading case also passed alone in 29.43s. No fixture timeout was changed.
Twenty declaration/compiler and 24 ordinary media cases passed earlier in the
same isolated target. This is 66 distinct passing local tests; repeated reruns
are not counted again. Final all-target/all-feature Clippy passed with warnings
denied (3m33s). Formatting, whitespace, actionlint and local documentation links
passed.

The repeated CLI identity probe after the targeted hash profile change took
1.78s for a 45,926,912-byte executable, versus 2.97s before; both intentionally
refused missing alignment and created no output. The compiler probe includes
startup/metadata work and is not a standalone cryptographic benchmark.
Application assertions and per-operation source/runtime identity checks remain
unchanged. Final hosted checks must cover the latest main, which advanced with
workflow PRs #393/#396 while the first M10 cycle ran.
## First hosted checkpoint and current-base qualification

Implementation head `87eb0ec15c57a6e28b45dcef50a35328325fd8ec` passed
[full CI](https://github.com/giodl73-repo/REEL/actions/runs/44978510415)
in 16m49s and
[source/assembly qualification](https://github.com/giodl73-repo/REEL/actions/runs/44978510376)
in 5m15s. Both platforms executed the companion metadata step; Windows also
executed the native SVG/PCM step. Intentionally unenrolled platform steps are
not counted as executed tests.

The branch was subsequently rebased onto `dc16335` (#393/#396). Their guarded
film/workflow changes are preserved. A fresh complete hosted cycle is required
on that combined tree before merge; the first cycle alone does not qualify it.
