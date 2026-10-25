# Native alignment metadata inspection

`verify_native_alignments: true` opts a scene context into selected native
alignment JSON inspection. The generic inspector shares the selected metadata
admission path with source-text checks, and uses shared native alignment
validation and semantic-event span compilation. See [contract](../asset-references-v1.md).

Exact cue/language/take identities, clocks, all marker bounds and native event
ordering/coverage are checked. Lane rates must agree and totals use checked
integer sums. Segmented-take cues are explicitly held. Failed language results
hold the context; native resolution, source-text and presence stay separate.

No audio decoding, duration measurement, provider accuracy, presentation-line
marker timing, template layout, rendered timing or human approval is inferred.
All preexisting coverage findings remain. The stricter shared alignment validator
also rejects unused markers outside the cue; historical metadata is not rewritten
to pass. Private failures belong in the producing project's evidence/backlog.

Review: [producer-applied checklists](../reviews/2026-10-21-native-alignment-inspection.md).
Private fixture reports/runtime/JSON stay in consumer cache/LFS, with only compact
profiles and exact pointers in Git. No new cache, daemon, service, lifetime lock,
archive crawl, hydration or media mutation. Next: selected template/presentation
clock validation, keeping actual decoded/render acceptance separate.
