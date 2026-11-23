# Reuse completed episode source identities

The optional reuse path keeps the journal's identity, command, byte and history
bounds unchanged while allowing an episode producer to version its own completed
component identities. Media planning, current-reference verification, execution,
retention and approval boundaries remain unchanged.

An optional private episode profile field, `reuse_children`, names the exact
retained selection currently held by the same root source at `expected_revision`.
It must be a completed episode handoff with the same source and owner, a complete
component set, and fully withdrawn or aborted temporary sources. Each temporary
source must still have its recorded owner, no current or pending claim, and the
revision immediately after that handoff's retirement.

The controller derives identities from that admitted handoff. It does not accept
arbitrary substitute identities, reuse live claims, remove old versions, create
a journal, or increase a guard. A changed component set requires separate normal
admission; it cannot silently allocate an additional identity in this mode.

Each reused temporary source advances from its retired revision through fresh
prepare/adopt and retirement commands. The new operation has its own command
identities. The original handoff, selections and all predecessor obligations
remain retained. The frozen worker ticket and new retained handoff bind the exact
reuse selection and derived identities/revisions, including failure recovery.

Omitting `reuse_children` preserves the existing new-identity behavior. An
unchanged completed profile may recover only its own retained handoff; changing
the reuse selection, owner, revision, component set or operation must refuse.

Unit qualification covers default compatibility, same-owner retired-state
admission, stale/live/foreign/incomplete pool refusal, partial failure cleanup,
and idempotent metadata-only recovery. The explicit native integration fixture
runs a four-part episode, fills its temporary journal to `MAX_SOURCES`, confirms
that a fresh identity is refused, then renders again using the original retired
component identities. It checks the native frame/sample clocks, selected media,
decoded picture and PCM digests, predecessor preservation, and unchanged prior
movie bytes. The root advances from revision 6 to 12 and the reused children
advance from revision 3 to 6; a new identity remains refused afterward.

Run that fixture explicitly with the native media tools on `PATH` and an
explicitly pinned `REEL_TEST_TIG_PROPOSE` executable:

```text
cargo test --locked --features tig-publication --test tig_inputs completed_native_episode_reuses_retired_components_at_full_source_guard -- --ignored --nocapture --test-threads=1
```

The fixture uses generated inputs in its own temporary storage. It does not
qualify a private project's runtime adoption, creative approval, or publication.
No private media or operational identities belong in these reusable tests.

Producer-applied review covers the editing and rights/provenance checklists.
Editing: the option changes claim identity admission and retirement; it does
not change scene order, timing, reference comparison, or media execution.
Rights/provenance: each run retains its own exact context and previous resource
obligations, while approval and production-claim boundaries remain unchanged.
These are technical findings, not independent review or human acceptance.

Concurrency and recovery checks also require the exact same owner and source
revisions before preparation and again under the root fence. Reused component
cleanup acknowledges this run's exact prepare/adopt operations before retiring
claims. A full identity journal remains full: reuse versions existing actors
and does not free, replace, or conceal an older actor or predecessor.
