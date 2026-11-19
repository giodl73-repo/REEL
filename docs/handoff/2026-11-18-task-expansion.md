# Task expansion from retained results

`reel.workflow-expansion-template.v1` pins the complete base plan's canonical
SHA256 and describes one new task. Inputs are explicit fixed artifacts or parent
selectors (`output` by name, or `worker-receipt`). Deterministic templates also
assign those inputs to publication, request, source-path and optional part-plan
roles. Neither future output hashes nor placeholder artifact identities are needed.

`reel_workflow::expansion::prepare` uses the existing verified graph observer.
Every named dependency must be observed as the exact retained original job and
pass the adapter's actual protection/custody verification. Waiting, pending,
cancelled, stale and refused jobs do not unlock expansion. Fixed and parent
origins cannot alias one logical input even when their bytes match. The result
preserves existing tasks and adds the concrete task, packet and upstream binding
control with full parent-result fingerprints.

The native command is:

```text
reel-workflow-native expand-plan PLAN RUN_BINDINGS TEMPLATE NEW_PRIVATE_EXPANSION --apply
```

It verifies parents through the existing native observer and writes one new,
bounded private control file, with guarded parent-directory access, exclusive
creation, flushing and readback. It refuses output in loaded native/control or
source namespaces. Partial writes remain preserved. This is a planning artifact;
it neither runs the task nor admits its inputs, registers Git records, selects
assets or changes a selected branch.

The proposed task intentionally has no native input declaration yet. The
controller must next freeze its exact Git/composite source profile, add the
matching declaration and independently admit/recheck its actual parent inputs.
The expansion packet's planning key must not be reused as a completed/admitted
key after those fields change. Sequential observations are not an atomic custody
snapshot or permission to release parent history. The 128-task/input bounds,
1 MiB control limit and 32 MiB selected-parent evidence budget remain explicit.

All 57 workflow tests and strict shared Clippy pass. Five new protocol cases
cover exact outputs, tool-request/source roles and worker receipt selectors,
unretained/refused states, origin aliases, stale bases/originals and missing
selectors. All 38 default native tests pass. One native CLI test covers new-output preservation, explicit apply
and unretained-parent refusal; it supplies no native custody. Strict native Clippy
passes. A separate positive native check reopens the qualified synthetic issue
through its frozen profiles and bare Git history after source-actor coverage loss,
then expands an unadmitted review task from its exact transcript and worker receipt.
The exported full parent-result fingerprint, both artifact identities and all
unchanged existing tasks were independently checked against original controls.

Evidence is in `F:/src/tig-dev-temp/reel-workflow-native-expansion-20261118-v1`.
The clean source commit is `072ac9b`; all 1,810 archived files match, with source
manifest SHA256 `64e3e944f9a025224970fa4222c9c87832efb8be10aea85768284364b830e3dd`.
The frozen runtime SHA256 is
`b3388e6b6e6fdc183d342f3ed8c41bdd6c49e1065f0a17811bb6ca0a5e7a79ee`.
This qualifies bounded expansion metadata from actual historical custody, not
admission, provider execution or retention of the newly proposed review task.

Producer-applied provenance/recovery checks preserve exact origins, original
attempts, unchanged existing tasks and separate custody/approval decisions.
This is a controller primitive, not complete automatic recipe scheduling,
capacity/actor allocation, guarded retirement or consumer adoption.
