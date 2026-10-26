# Compiler asset-ID representation review

Producer-applied editor, sound-designer and rights/provenance checklists; these
findings are not independent review or publication approval.

- Editor: source IDs come from selected scoped bindings. The explicit profile
  mode preserves native event timing and exact media pins. Legacy profiles retain
  their cache-relative source representation.
- Sound designer: cue, score and sonic production sources use the same mode as
  job FileRefs, including placement evidence. No timing or mix change is intended.
- Rights/provenance: a logical ID is not a filesystem path or approval. The Tig
  consumer still captures an explicit commit and verifies the expected object.
  This change adds no cache, transfer, deletion or implicit latest lookup.

The synthetic compiler regression compares picture/audio instructions against
legacy compilation, preserving pins and timing while checking the source IDs.
Unknown profile modes refuse. Existing native-provider tests cover exact Git
views, stale pins and protected render access. Automatic native control
materialization and refreshing bindings from the captured view remain separate.
