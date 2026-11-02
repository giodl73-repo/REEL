# Pulse 4: preserve moving production main without freezing unrelated work

Production PR 297 merged as 0e34cc35 while this browser client qualified. Integrate
that current main without overwriting the owner's browser-presentation/native
TIG changes or native CI additions. `crates/reel-assembly`, Cargo.toml and
Cargo.lock are unchanged from entry e4d22e0. The client continues to use the
same portable core and all 24 shared production dependency identities.

The original Pages guard compared all production src/vendor/workflows against
the historical entry forever. That would make unrelated legitimate TIG changes
fail the assembly Pages workflow. Scope the ongoing frozen-source guard to
`crates/reel-assembly`, which owns this client's actual contract. Production
preservation is still checked against integrated main for this change; original
Linux/Windows/FFmpeg CI is retained byte-for-byte and continues running.
Native CLI controls are rebuilt from the integrated source. Whole-repository Git
source is still packaged and verified exactly, not reduced to the frozen slice.

Agent ownership review: maintain the portable kernel boundary without taking
control of another session's production implementation. Do not claim that the
browser inspects media or exercises TIG delivery. Re-run qualification after
integration and recheck public main before merging; do not waive native checks.
