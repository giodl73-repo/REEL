# Replay a bilingual briefing

M14 is staged against the branch contract. Final execution checks follow
the merged [branch contract](interactive-branch-contract-v1.md).

The listener hears the opening, then chooses a worked example or a summary.
The worked example plays the technical detail and closing. The summary plays
the executive closing. A text ending is shared by both routes. Use the
[bilingual engineering fixture](media-expansion-spoken-fixture.md) and match
each selection to its independently checked programme and transcript.
The [replay fixture](../tests/fixtures/spoken-programme/interactive-replay.json)
records the expected visits and sample ranges for the local test.

| Visit | English edition and sample interval | Spanish edition and sample interval |
| --- | --- | --- |
| Opening | Executive `[0, 144000)` | Executive `[0, 168000)` |
| Worked example | Technical detail `[158400, 350400)` | Technical detail `[187200, 403200)` |
| Example closing | Technical closing `[364800, 508800)` | Technical closing `[422400, 590400)` |
| Summary | Executive closing `[156000, 300000)` | Executive closing `[184800, 352800)` |

Sample ranges are half-open and use 48 kHz edition-local clocks. Bind a visual
companion for the worked example only when its receipt matches that exact
edition and interval. The [companion guide](semantic-visual-companion-v1.md)
describes the separate phrase and frame evidence. Source fixture audio contains
controlled signals and the test transcript derives from the fixture script.

Run both routes in both languages. Repeat each input trace and compare visit
order, selected artifact identities, accepted media positions and closing
state. Include keyboard and text activation of the same choice ID, Pause and
Resume inside the detail, and a late end observation addressed to the opening.
Changing language should create a fresh opening visit with fresh choices.
Removing the selected branch or its exact access binding should prevent
execution. Closing the active session should discard its interaction state.

The local session reducer admits every authored binding through the checked programme API before entering the graph. Its replay test drives both routes with confirmed frame observations, tests the shared choice IDs, and compares exact admitted artifacts across repeated runs. The local PCM adapter reopens the checked WAVE at each playback visit, submits exact stereo PCM24 spans to a sink, and advances only after the sink acknowledges a chunk. The test sink records delivered byte counts and digests. Speaker playback and keyboard handling still need a local frontend.
