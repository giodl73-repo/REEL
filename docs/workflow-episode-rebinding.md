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

The acceptance fixture needs distinct ES and EN scene contracts and recordings,
a shared repaired cel, and an unaffected component. The fixture should verify
both actual episode conforms, their unchanged segment order, the exact reused
components, language-specific frame/sample receipts and captions derived from
those receipts. Reject a result from another language, a stale baseline
component, a mismatched worker receipt or missing retained custody before
assembly. Recovery must reopen the original assembly attempt and its outputs.

The first fixture can use short episodes. S5 acceptance additionally needs
the existing producer's long-running resource and capacity path, including
the multi-gigabyte opening and ending, and the required delivery editions.
