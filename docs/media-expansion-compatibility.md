# Spoken source across delivery formats

M15 design draft based on stacked commit `c9b2c71`. Final qualification runs
after M14 merges and records the exact checked artifacts and evidence.

The [bilingual fixture](media-expansion-spoken-fixture.md) authors `opening`,
`detail` and `closing` once as module identities. Each language selects its
own script and takes. The executive edition omits `detail`, and the technical
edition includes it. A delivery package can reference a checked edition and
its local sample bounds while keeping its own artifact identity.

| Delivery | What it binds | Current evidence and remaining work |
| --- | --- | --- |
| [Spoken edition](spoken-edition-check-v1.md) | Selected language, exact takes, PCM, transcript and module chapters | Controlled English and Spanish fixtures check independent PCM and transcript derivation. [History](spoken-edition-history-v1.md) covers exact retained identities. |
| [Narrated reading](narrated-reading-v1.md) | Checked chapter intervals and authored page/text spans | M08 merged with engineering evidence for the reading index. The final cross-format run must link the index to the selected common edition. |
| [Lesson and rehearsal](lesson-rehearsal-v1.md) | Explanation, selected audio, editable score, notation exports and passage occurrence | M09 merged with checked lesson fixtures. The final run must demonstrate the common edition relationship and preserve the score and occurrence identities. |
| [Visual companion](semantic-visual-companion-v1.md) | Authored events, exact edition phrases and the matching visual clock | M10 merged with a one-language replacement case. The final run must bind the common selection and verify the unchanged language's evidence. |
| [Offline installation](offline-installation-v1.md) | One whole checked edition, matching text and an enrolled runtime and endpoint | M11 defines the contract. M12 station fixtures use an independently admitted quiet-seam signal. Any shared-source demonstration must satisfy the seam and station gates. |
| [Interactive branches](interactive-branch-contract-v1.md) | Authored routes and exact edition, module, media and access bindings | M13 has preparatory bilingual admission tests. M14 execution and deterministic replay are still required. |

The final run should retain the original edition and each derivative by its
own checked identity. It should follow at least one selected module through
all published relationships, exercise a changed take and refused input, and
link accessibility evidence. A quiet-seam derivative needs an authored and
checked relationship to the selected source. Listening, device acceptance,
authority and project adoption each require their own evidence.

### Installation seam in the shared-source run

The baseline fixture starts each take with 2,400 silent samples at 48 kHz.
The installation contract requires 4,800 silent sample frames at both ends of
the admitted range. For the shared-source run, author a new synthetic take
version with sufficient leading silence, then compile and check its affected
editions. The baseline executive transcripts exceed the terminal line profile
even after the audio seam is fixed. Author a dedicated opening-only edition
per language in the same programme, selecting the same checked opening take
as the executive edition. Installation then admits a whole edition and the
other deliveries can continue to follow the shared module and its chapter
bounds.
The [existing installation test fixture](../tests/support/spoken_installation.rs)
already generates a quiet-seam variant and uses stereo markers to identify
its boundaries. Its shortened, opening-only programme covers the station
profile. Reuse that signal construction for the selected take while preserving
the full shared programme needed by the other deliveries.
Retain the baseline fixture and version history so the changed source and its
consuming sample intervals can be compared directly. Obtain the station and
listening evidence for the selected version separately.
