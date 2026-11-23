# Rebinding a repaired scene into an episode

The episode producer checks selected scene outputs against the current source
recipe and an original, replayed producer claim. A retained workflow film result
establishes custody of its output bytes and binds them to its worker receipt.
It does not currently supply the scene producer claim required by
`tig_episode::check_current_scene`. Changing a master hash in the episode
manifest alone therefore cannot make the repaired scene eligible for assembly.

There is also an existing selected-master reference route. The episode worker
calls `ProtectedReference::verify_selected_master` and passes the resulting
`VerifiedScene` into the protected composition plan. `ReferencePlan::compile`
binds the selected output record, scene ID, language and segment ID, then
compiles the current native job, contract, production manifest and semantic
delivery controls. This is the integration point to qualify for a workflow
master after explicit selection. The reference verifier must independently
accept that master against the freshly compiled controls before assembly.

Use the existing episode producer as the integration target. Keep the episode
conform and its ordered segment IDs as the baseline. For each affected edition,
bind the segment ID and language to the exact old component and the new retained
render, including its worker receipt, scene job, delivery receipt and master.
Validate the repaired scene against the current selected source recipe through
the selected-master reference verifier before compiling new episode controls.
Use the existing protected composition path for the resulting controls and
guarded inputs. Reuse unaffected scene and presentation components through
their existing verification paths.

## What the episode worker now verifies

The protected selected-master tests run the repair and episode assembly
separately for Spanish and English. Each verifies the accepted language,
unchanged scene sample and frame counts, byte-identical dialogue, music,
effects and mix, and exactly one changed episode component.
The English lifecycle also assembles an episode using its own cue and
alignment. These are independent synthetic projects.

## Shared repair across editions

Use one committed baseline and one retained image candidate to prepare
all affected language tasks. The [native rebuild binder](workflow-native-film-rebuild.md)
already emits the exact packets and source declarations for those tasks.
The caller still supplies each owned dispatch request and recovery ticket.
Use each render's retained result to bind the corresponding episode segment
before compiling its episode controls. Reopen an existing ticket on
interruption and bind any completed result by its original operation.
Withhold each downstream consumer until its new exact inputs are available.

The remaining fixture should exercise that sequence in one project with
distinct Spanish and English cue clocks and at least one unaffected
component. Check exact reuse, unchanged segment order, rejection of
wrong-language or stale components, and captions derived from each retained
conform. S5 acceptance also requires large opening and ending masters,
long-running recovery and the project's delivery editions.
