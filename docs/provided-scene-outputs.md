# Check scene outputs through a verified-file provider

`scene_delivery::check_with_output_files(job_path, inputs, outputs, receipt_ref)`
uses the existing `InputFiles` interface for both primary sources and delivery
outputs. The exact receipt reference is explicit; no receipt filename or sibling
directory is inferred. Outputs may live at unrelated cache paths without copying
or linking them into a worktree. This is a Rust API, not a runtime configuration
switch or complete Tig output-bundle enrollment command.
An explicit receipt larger than 4 MiB refuses before any provider or planning
access, so a movie cannot accidentally be loaded as receipt metadata.

The output provider must verify each expected hash/size and retain effective
byte/path protection for the entire synchronous check. A bare cached path is
insufficient for concurrent production. As with primary providers, this is a
trusted adapter contract, not a sandbox. Legacy local-root checks retain their
existing containment, exact case and file-hash validation.

The shared checker validates the current compiled job/plan, receipt schema and
sample/frame counts, exact output set and each output's declared relative name.
It resolves every declared output through the provider before decoding. Camera,
overlay, PCM, frame/geometry/codec and indexed Motioncraft checks then use only
that resolved map. Missing mappings fail; no local-root fallback occurs. Tool
selection stays with the primary provider. Output locations cannot select an
alternate FFmpeg executable. Receipts keep original logical output names and
contain no provider filesystem paths.
Indexed snapshot images are decoded explicitly as PNG, preserving their logical
format contract even when a cache filename has no image extension.

`check`, `check_with_inputs` and the internal native receipt recheck use the same
implementation. The public provider API always performs full media checks; it
does not expose the internal producer-proof-based reduced recheck. It does not
grant current-source producer correspondence, selection, creative approval,
independent custody, placement or eviction authority. Provider guards are temporary
read protection, not persistent retention demands. Native episode execution still
needs its own source/job/presentation/template binding and bundle adapters.
