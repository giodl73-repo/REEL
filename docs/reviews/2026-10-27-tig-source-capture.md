# Committed-source capture command

Entry64110ee38c2e1547fe8f533526208a9854fb4201. Tig WP-003/010/011/016,
IF-005/010/011/012, PKG-003/004/006. Explicit caller action only: capture an
admitted committed source, compile and freeze its native render context into an
existing metadata store, finish/reopen/recompile/read back, then publish a small
private worker-context fragment without overwriting an existing file.

Reuse existing capacity admission and owned preparation cleanup. Inspect validated
metadata limits before reservation; recheck writer limits before writes. Reserve
database plus rollback allowance on an explicitly configured active root on the
metadata/output volume. Compiler scratch has its own shared reservation. These
are cooperative capacity reservations, not filesystem quotas. No new per-job DB,
source journal, cache, runtime installation, render, adoption or eviction.

L0 fmt, locked lint, default-feature isolation. L1 synthetic committed source,
bounded capture/readback and exact fragment replay, existing-output refusal,
capacity refusal before provider writes, malformed request/source refusal and
owned scratch cleanup. L2 supported CLI capture using the existing generic native
fixture; actual media rendering remains separate qualification.

Producer rights/provenance lens: raw profiles, paths, identities and contracts stay
private and outside Git; stdout is count-only. Editor/sound lenses: use the existing
compiler and compare exact original/replayed context requirements, descriptor and
selection. No creative approval, independent review or full migration claimed.

## Executed qualification

Vendor exact Tig4715559021c48bbdebc422d5047b01bce62a99d8: 61 upstream files,
656,167 bytes, unchanged MIT license. This includes validated reader/writer bounds
and a boxed private SQLite authority variant for Linux enum-size lint. Upstream
PR41 requires exact Linux/Windows qualification before this consumer is merged.

All 54 ordinary native integration tests pass (nine explicit external cases
ignored). The new cases exercise no-overwrite/capacity/source/request refusals,
unchanged provider bytes on those refusals, concurrent capture with scoped retry,
preserved unrelated scratch, and two versions differing in one cel replayed with
Git/records/definitions physically unavailable. The first regression found stale
expected vendor identity/byte-count constants; those were updated after verifying
every vendored file against upstream, and the complete suite then passed.

The existing external compiled-CLI fixture now starts through the capture command.
It passes capture, planning, managed FFmpeg rendering, retained record candidates,
source-Git-offline execution, explicit Git proposal/check/application and its
existing negative cases (178.86 s). Raw fixture media remain synthetic.

Locked all-target/all-feature warnings-denied Clippy and no-default-feature
library isolation pass. An actual current private committed scene was captured
in both Spanish and English with the supported command: each context recompiled
and read back with 30 primaries, five controls and a 1,883-byte private fragment.
Those rehearsals read named JSON controls/alignment only, with no primary image
or audio decode, rendering, runtime installation or production cache changes.
This is scoped consumer evidence, not all-family migration or global custody.
