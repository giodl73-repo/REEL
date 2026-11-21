# Spoken programme engineering qualification

**Status:** M06 engineering scope on merged M05 `027164f`, [PR #383](https://github.com/giodl73-repo/REEL/pull/383).
Final local and hosted results are recorded in the [review](reviews/2026-11-21-m06-spoken-qualification-review.md).
The spoken checkpoint is **awaiting listening
evidence**: no approved voice profiles or actual EN/ES recordings have been
supplied for this work.

## One module, two speakers

The baseline four-edition fixture retains its original clocks. A separate
variant adds a host question before the expert answer inside `detail`:

| Cue | English | Spanish |
| --- | --- | --- |
| `detail-question` / host | How does an edition choose its material? | ¿Cómo elige una edición su contenido? |
| `detail` / expert | Each edition selects exact takes and declares its speaker handoffs. | Cada edición selecciona tomas exactas y declara los cambios de hablante. |

This is authored public test prose and controlled PCM signals, not recorded
speech, approved translation or an actual conversation. The question has 72,000
EN samples / 96,000 ES samples. The original expert answer remains exactly its
baseline take. The question→answer added pause is 4,800 EN / 7,200 ES samples;
the opening→question and answer→closing pauses are explicitly declared as
14,400 EN / 19,200 ES samples each. All inputs retain their recorded leading and
trailing zeros. Signal codes 7 and 8 extend the original fixture's formula.

| Edition | Opening | Question | Answer | Closing | Samples / seconds |
| --- | --- | --- | --- | --- | --- |
| EN technical | [0,144000) | [158400,230400) | [235200,427200) | [441600,585600) | 585,600 / 12.2 |
| ES technical | [0,168000) | [187200,283200) | [290400,506400) | [525600,693600) | 693,600 / 14.45 |
| EN executive | [0,144000) | omitted | omitted | [156000,300000) | 300,000 / 6.25 |
| ES executive | [0,168000) | omitted | omitted | [184800,352800) | 352,800 / 7.35 |

The detail chapter spans both cues and their internal gap. Transcript entries
keep the host as Speaker 1 and expert as Speaker 2, with engineering-signal
labels and explicit added-pause entries. Selected scripts remain separate in the
declaration and consumed recipes; signal labels do not pass them off as spoken
transcript text. No word-evidence rows are manufactured.
The executive editions keep their original bytes and consumed recipes. Their
opening→closing adjacency is checked directly; it does not inherit either
technical handoff.

## Expected checks before execution

Use the existing M01–M05 contracts, 48 kHz stereo PCM24, half-open samples,
integer math and exact decoded-sample equality. The engineering profile's
720,000-sample output, 16 MiB source PCM, 64 MiB output and 128 MiB workspace
bounds stay fixed. Each producer/checker operation retains its cooperative
30-second budget and the fixture's measured 60-second outer budget.

`tests/support/spoken_multicue.rs`, included in `spoken_programme_media`, must:

- compare every decoded sample with a hand-placed signal oracle and all four
  transcript/chapter clocks with the table above;
- write and reopen declaration, canonical plan, candidate receipt/audio and
  access manifest/transcripts as a reproducible temporary engineering pack;
- compare baseline and variant recipes, preserving the two executive identities;
- reject a reversed/missing cue, reused technical adjacency in an executive
  edition, protected-word overlap and unresolved context before production; and
- check a separately authored reordered technical edition against its own
  sample clock, refusing its candidate under the original controls.

Reordered variant: opening→closing→detail(question, answer), with EN added gaps
9,600 / 7,200 / 4,800 and ES gaps 12,000 / 9,600 / 7,200. Expected EN spans are
[0,144000), [153600,297600), [304800,376800), [381600,573600); ES spans are
[0,168000), [180000,348000), [357600,453600), [460800,676800). This tests explicit
ordering only; it does not establish that the reordered explanation is coherent.

```powershell
cargo test --locked --test spoken_programme_media qualification:: -j2
cargo test --locked --features tig-history --test spoken_programme_history --test spoken_programme_media --test spoken_programme --test cue_relative --test audio_event_clocks --test v0213_cli -j2
cargo clippy --all-targets --all-features --locked -j2 -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The engineering pack is recreated in a test-owned temporary directory, checked
there and removed by the fixture. It is not an installed or delivered media pack.
No production cache, provider, voice, render or upload is used.

## Separate spoken checkpoint

Real demonstration needs producing-project references for approved host/expert
voice profiles, exact source/script/translation and reuse authority, admitted
recordings, measured per-language word evidence and original runtime pins.
Then apply the [listening rubric](media-expansion-test-matrix.md#human-review-rubric-and-gate-handling)
to complete programmes and every changed adjacency: word fidelity, language,
speaker identity, breaths/tails, natural pauses, masking, loudness/peak and text
access on the named devices. Record exact bound review and selection artifacts.

Until that evidence exists, listening, acoustic word fidelity, voice consent,
selection, delivery and adoption remain **not run**. Later engineering fixtures
may reuse the technically checked interfaces but cannot inherit those gates.
