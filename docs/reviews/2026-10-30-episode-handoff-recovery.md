# Interrupted episode context handoff

A stop after durable handoff prepare previously left a pending root that
`finish-episode` refused. The command now rebuilds the complete retained packet
from the original successful result, its input context, the generated parent and
still-current child owners. Publication and independent metadata readback happen
again before comparing the rebuilt selection with the pending selection.

The original prepare command is replayed with its original revision and phase.
Its acknowledgment must match the pending reference. A foreign preparation,
even for the identical packet, cannot be adopted through this recovery route.
Only then does the normal adoption and temporary-owner withdrawal proceed.
Completed handoff selections also bind their context identity to the header.

The synthetic metadata fixture interrupts the actual handoff after prepare,
reopens the journal/provider, verifies budget and missing-report refusals leave
the journal unchanged, restores the report and resumes to the identical pending
selection. It checks all temporary claims are withdrawn only after root adoption
and that repeating finish makes no journal changes. A second fixture tests a
different pending packet and a foreign prepare for the same packet. Both preserve
every claim. Tool paths deliberately do not exist; recovery never binds them.
These Windows tests use the same prevent-mutation report reader as the command.

Final local qualification passes strict locked all-target/all-feature Clippy,
226 enabled library tests, formatting and the full episode CLI fixture in
130.83 seconds. Hosted qualification is still required.

Producer-applied provenance review requires exact phase/owner/revision checks,
complete context readback and retained historical evidence. Editor and sound
review retain the native frame/PCM/timestamp checks from the successful worker;
recovery neither rerenders nor substitutes a new creative decision. These are
checklist findings, not independent approval or production acceptance.

The scope is a successful job interrupted during context handoff. Failed workers,
earlier runner-retention interruptions, output-record/Git proposal integration,
generation/text freshness and actual project cutover remain separate acceptance
work. No payload deletion, backup custody or eviction authority is added.
