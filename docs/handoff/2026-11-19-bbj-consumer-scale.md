# BBJ comic consumer scale and source boundaries

A read-only observation of BBJ's selected Sightshift issue 02 preview v009 finds
20 selected story pages, 220 declared layer rows and 44 dialogue IDs. The source
control says 22 story pages with two remaining; it does not represent a delivered
whole issue. The selected preview is 27,400 bytes, SHA256
`ff30d00b973481caeb4fdc0cb8a85a0ac59f61255d59b443c42bf52303a826d4`.
The active pointer's exact typed selection matches that pin. No active source,
selection, delivery, server, cache or production job was changed.

The 100 selected page-row pins declare:

| Artifact role | Count | Declared bytes | Largest declared object |
| --- | --- | --- | --- |
| Page source JSON | 20 | 82,329 | 4,502 |
| SVG | 20 | 537,389,301 | 39,650,410 |
| Reading PNG | 20 | 65,060,707 | 3,799,436 |
| Authoring JSON | 20 | 246,740 | 19,451 |
| Composition notes | 20 | 15,297 | 1,353 |
| Total selected-row artifacts | 100 | 602,794,374 | — |

These are authored declarations, not fresh native payload reads. They exclude
root-level controls/script, raw layer primitives, fonts, custody closure and the
remaining story/bookmatter. SVG embedding and standalone PNG are distinct
authored artifacts; this table is not a count of unique decoded image memory.
No images/SVG payloads were opened, rendered or copied. Only small named source
controls were read and privately retained. The first page's 3,733-byte source JSON
matches its selected byte pin and uses `bbj.layered-page-source.v1`, with ten
layers. That is not REEL's `reel.comic-page.v1`; no implicit schema conversion or
producer qualification is inferred.

Evidence is retained at
`F:/src/tig-dev-temp/reel-bbj-comic-scale-20261119-v1`.
`scale-observation.json` SHA256:
`ac6ada6cf75848549fba14bd62b08f547da045e6642c45abeaecd0ea5387d18f`.
`source-context.json` SHA256:
`480a2ac96ed19aee9930fdbe842d85a2be8f6e2f694be95ed90b557cf4bc30d8`.
These pin the selected preview, declared-row inventory, active pointer, review,
first source JSON and the three project contracts below. This is a working-tree
control observation, not a clean committed production snapshot or native custody
proof; earlier project checkpoint claims are not refreshed by this observation.

## Consequences for REEL

The 64 MiB comic input ceiling does not cover this complete selected-artifact
set. Even the selected reading PNGs alone approach it, before finishing the
edition or capturing editable provenance. The current synthetic qualifier's
32 MiB native reader profile also would refuse the largest declared SVG; a
project-owned profile needs its own explicit qualified allowances. Those are
comic/project bounds, not universal film limits. Raising numbers or omitting
required sources does not qualify bounded production.

Larger comic qualification needs explicit parts and complete fixed/upstream
closure, preserved source IDs, bounded file/stream composition and original-job
recovery at the actual declared scale. Reuse admitted media and exact source
versions; do not regenerate selected pages to fit the fixture. Determine actual
primary closure and native record/member associations before planning admission.
The existing two-page issue/review-control qualifiers establish none of these
full-project gates.

BBJ's `production/comic-archive-contract-v001.md` records a package that rebuilt
its canonical pages but lacked the explicit clean PNG needed for an edited
dialogue restore. Qualification must rebuild clean art, reading output and a
future lettering edit from a fresh owned restore. Its native-reference contract
also requires exact association between adjacent typed pins and explicit asset
IDs: matching bytes or successful source rendering alone is insufficient.
Keep failed versions and exact unchanged inputs; never rewrite admitted metadata.

The creator-directed comic review standard retains plausible visible continuity,
connected anatomy, separated editable layers, actual visual review and honest
edition scope. It supersedes engineering-style apparatus/body gates. Generic
workflow qualification must preserve that project authority rather than adding
new creative/engineering conditions. Technical source restoration is not canon,
translation, art approval, print readiness, animation readiness or release.
