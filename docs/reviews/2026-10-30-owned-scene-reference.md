# Current-source ownership for retained scene masters

`tig_scene_production::jobs::reference::ReferencePlan` binds a selected scene
master to a current native job without asserting a historical render claim.
The explicit output definition, scene relationship, language and segment must
match the captured source. The master is a composition resource, never a primary
input to its own render.

```text
captured source + native job + selected output record
                         |
                compile small controls
                         |
       freeze -> finish/reopen metadata -> protect source
                         |
        hold current primary, output and runtime byte guards
                         |
       stream native reference pixels + compare complete PCM
                         |
               sealed VerifiedScene
```

Controls use the same bounded Merkle framing as current episode controls.
Readback requires every physical control node and the pinned runtime manifest.
The durable source obligation includes those nodes, the output record/definition
and exact selected master. Controls survive unavailable preparation and source
checkout directories. No receipt is invented or historical approval modified.

The returned proof borrows its protected source and runtime and owns all primary
and output byte guards. Currentness checks include the journal revision and exact
captured scene/output references. Separate source owners remain independent;
withdrawal invalidates the matching proof. Dropping a proof does not withdraw
durable obligations. Its declaration is an observation, not a capability that
can be deserialized to skip verification.

Production callers must reserve staging capacity and run inside the bounded
worker. This module does not install runtime bindings, schedule work, authorize
eviction, grant creative approval or establish independent backup custody.
Parent compilation/assembly must explicitly accept this proof and retain the
complete child resource and ownership closure; that integration remains separate.

Producer-applied editor, sound-designer and provenance checklists require exact
pixel/audio clocks, live byte guards, unchanged historical assertions and clear
technical/creative boundaries. The tiny episode fixture exercises offline
controls, missing-node refusal, wrong output/type refusal, concurrent owners,
mutation exclusion and withdrawal. It does not qualify all real project scenes.
