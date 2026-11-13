# Explicit dependencies in frozen generic jobs

The frozen capture/run CLI previously exposed only ordinary job admission.
Retained components and evidence could be frozen through the Tig SDK, but a
caller could not declare those roles through the CLI. Putting those objects in
Runtime correctly failed when the worker tried to consume them as inputs.

Version-2 capture and run profiles explicitly admit Composition/Evidence maps
through the existing SDK and managed runner. Version-1 admission stays unchanged.
No new renderer, cache, source transition or producer-claim mechanism is added.

Producer-applied review:

| Requirement | Finding |
| --- | --- |
| Preserve resource meaning | Dependency maps replay with their exact roles, names, hashes and sizes; objects are not promoted to Primary or Runtime |
| Keep old admission bounded | Legacy profiles refuse dependency declarations; version 2 requires a nonempty declaration within the same profile/tree bounds |
| Preserve supported platforms | Capture still requires the existing Windows allocation geometry; the non-Windows fixture checks portable role replay and capture refusal without metadata or journal mutation |
| Preserve ownership on refusal | Existing owner/revision/selection checks and scoped fences remain; invalid-version, missing-declaration, changed-role and altered-size fixtures leave the journal unchanged |
| Operate without source history | On Windows, the synthetic CLI fixture captures, adopts, hides the source repository, executes the component/evidence inputs and retains checked outputs |
| Keep claims separate | Generic success retains a null production claim and false creative approval; version 2 explicitly reports no original-producer currency assertion |

The integration fixture also checks that the ordinary runner refuses the same
dependency context, that capture does not change the source journal, and that
version-1 capture still omits the dependency field. These are technical producer
checks, not independent review, human approval or project delivery acceptance.
