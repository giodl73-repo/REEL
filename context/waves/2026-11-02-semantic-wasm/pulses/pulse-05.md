# Pulse 5: budget the loaded runtime separately from complete source

The complete source/legal artifact remains below 5 MB after production PR 297
integration, but most bytes belong to the optional whole-repository source ZIP,
source pins and original notices. Active native TIG work grows that source while
the assembly runtime stays unchanged. Budget these two delivery obligations
separately: initial runtime files plus root landing must remain below 512 KiB;
the entire exact source/legal site including inventory and landing is capped at
8 MiB. Both package staging and independent artifact verification enforce these
limits, and served provenance names them explicitly.

All Git source files and original notices remain included and verified. The
existing input, graph-count, report, 64 MiB memory and real worker/read deadlines
are unchanged. Native production CI is still required. The new runtime limit
measures what a visitor actually loads; the larger optional-source capacity
avoids making unrelated production-code growth a browser startup constraint.
