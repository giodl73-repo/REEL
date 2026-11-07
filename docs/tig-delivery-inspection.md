# Inspect frozen delivery controls

`reel-tig-job inspect-delivery PRIVATE_PROFILE` prints the three exact native
controls compiled from a frozen delivery-template context: `job.json`,
`contract.json` and `production.json`. Each has its relative reference, SHA-256,
length and exact UTF-8 bytes. The report also binds consumed primary names to
their immutable references. This reuses the existing native lowerer rather than
requiring a project to reproduce its compiler.

The strict profile schema is `reel.tig-delivery-inspection-profile.experimental-1`:

| Field | Admission |
| --- | --- |
| `metadata_root`, `metadata_minimum` | Existing frozen metadata store and trusted checkpoint |
| `source`, `context`, `request` | Exact archived source, context selection and delivery-template request |
| `producer`, `recipe` | Independently admitted original producer Object and expected recipe digest |
| `max_control_bytes` | Explicit sum bound for the three control bodies, at most 4 MiB |

The archive provider must require mutation prevention. Inspection replays the
frozen context and original recipe before returning controls. It opens no primary
media, adopts no source, creates no cache, launches no program and writes no files.
Stdout is private operational payload; retain it in the existing asset/cache
system. It is not small Git authoring data or permission to overwrite controls.

Later jobs must enroll the exact controls and primary bytes through the shared
lifecycle, independently check retained outputs and retain their real job history.
Inspection does not establish primary-byte availability, render success, semantic
selection, human approval, a producer claim, production currency or eviction rights.
Errors disclose fixed stage names only. Tests cover source and primary media
unavailable, exact control-byte pins, ordinary and draft review scopes, stale
context/request/recipe/producer refusals, bounds, weak protection, unknown claim
fields and unchanged metadata checkpoints.
