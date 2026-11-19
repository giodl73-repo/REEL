# Selected recording to semantic film controls

`film-scene-control` is an explicit deterministic workflow capability. It wraps
the existing `reel-assembly` scene compiler, authored language-local trigger
resolver and append-only semantic graph implementation. No provider execution,
recording, selected-ref change or movie render follows from generating controls.

The tool contract's `publication` is the exact scene-authoring V2 artifact;
`request` is a JSON `reel.workflow-film-scene-control.v1` artifact. `edition_id`
names the caller's edition, while task `language` chooses the scene lane. Source
bindings map portable paths to declared native/upstream logical inputs. Request:

```json
{
  "schema": "reel.workflow-film-scene-control.v1",
  "graph_path": "graph.json",
  "pointer_path": "pointer.json",
  "episode_path": "episode.json",
  "scope_paths": ["scene-scope.json", "episode-scope.json", "season-scope.json"],
  "cues": {
    "cue-1": {"text_path": "text.txt", "take_path": "take.wav", "words_path": "words.json"}
  },
  "next_lock_logical_id": "film/lock-v2"
}
```

Every supplied byte is checked against its declared Artifact. Graph lock content
is recomputed and the exact pointer must select it. Scene, episode and season
scope IDs and build states pass the existing compiler's checks. Every selected
take and picture must match actual declared input identity, hash and byte length;
a matching scope document alone cannot supply unavailable payloads. Source text
must match the scene cue's exact hash. Word evidence must match the selected take,
language, cue, spoken text and measured WAV sample clock. Trigger phrases come
from the scene's language event bindings; request authors do not supply seconds.

The shared mono WAVE clock supports PCM16/PCM24, including the corresponding
extensible format, bounded chunks and padding. It checks declared RIFF length,
format, byte rate, frame alignment and nonempty sample data without resampling.
Other formats are explicitly unsupported for this adapter. Existing recordings
can be declared directly; no synthesis/provider path is involved.

Required JSON outputs (each at most 1 MiB): `events.json`, `graph.json`,
`pointer.json`, `alignments.json`, `review-map.json`. A separate deterministic
`receipt.json` binds the original task/request/scene/inputs/outputs for native
worker replay. The candidate pointer selects the candidate graph internally;
applying it to a project selection still requires the separate approved selection
transaction. No working tree, index or selected branch changes.

The review map binds each event/cue and exact cel Artifact to start/end samples
and rate. These IDs can feed a future burned-in review copy and issue/fix lookup.
`rendered:false` and `approved:false` remain explicit: the map is not a film,
frame-location report, listening review or creative approval. Existing score,
VFX, motion and style authoring remain in the scene; this capability compiles
semantic event controls, not their complete audiovisual render closure.

Bounds: at most 32 cues, existing 128 declared-input/task contracts, 128 MiB
aggregate source binding bytes, 1 MiB JSON controls and the existing native actor
and changed-path limits. Undeclared, unused, conflicting or stale inputs refuse;
there is no latest fallback. Owned native dispatch uses the original pinned
runtime, existing protection, output replay and retained-result mechanisms.

Component evidence: ES and EN resolve `mira`/`look` to different cut samples and
different cue lengths. Candidate graph/pointer correspondence and deterministic
replay pass; stale take hashes/clocks, unselected takes, changed pointers, source
text mismatches, absent trigger phrases and altered input bytes refuse. PCM
clock tests cover 16/24-bit and extensible headers, padding, truncation, malformed
format/chunk lengths and duplicate data. Workflow/assembly suites and strict
Clippy pass. Native end-to-end qualification is recorded separately once run.

Producer-applied editor/sound checks preserve measured language-local rhythm and
selected take identity. Animation-direction checks leave style/motion feasibility
with the actual renderer. Rights/provenance checks retain source/selection scope,
private evidence and distinct human approval gates. Technical checks do not
select a creative master or authorize public release. Full film rendering,
cadence/style preview, burned-in cel IDs, review findings/fixes and project
adoption remain open.
