# M05 retained spoken editions: checklist review

**Status:** merged in [PR #383](https://github.com/giodl73-repo/REEL/pull/383)
as `027164f` after full [hosted qualification](https://github.com/giodl73-repo/REEL/actions/runs/44916089006)
passed. This is one assistant applying the repository's
role checklists, not independent review or human approval.

## Scope and findings

| Lens / finding | Disposition and evidence |
| --- | --- |
| Editor / a global registry edit must not invalidate every edition | Canonical per-edition consumption excludes unrelated takes and projects reuse scope onto the consuming edition. The native test replaces only Spanish detail, then compares all four compiled, execution and native fingerprints and exact review targets: one changes, three remain equal. |
| Music Reconstruction / retained claims must replay the original consumed scope | Each edition freezes its own SDK context with descriptor equal to the operation parameters. Reopening uses actual SDK `replay_with_payloads` and rederives the original plan, exact PCM and access text. Full source provenance remains distinct from consumed identity; complete semantic coverage is not claimed. |
| Rights / byte availability is different from authority and selection | Private receipts retain exact source, runtime, authority-pointer and reuse-pointer bytes. Opaque pointers are not approved. Technical flags remain separate from consent, listening, selection, adoption and release. Advisory review bindings use the existing strict review-record protocol. |
| Sound / an old waveform must remain independently checkable | The M04 checker reopens retained sources and every output, recalculating PCM and text against original controls. A changed take invalidates its consuming binding. A retained v1 candidate survives v2 source registration and the absence of Git/working metadata. No signal fixture is described as speech. |
| Platform/Audience / portability and delivery claims need boundaries | Native protected paths qualify on Windows only; cooperative reads explicitly refuse without byte IO or metadata writes. Linux does not inherit Windows protection. Native receipts are private; no new portable receipt or public delivery is introduced. |
| Rights + recovery / publication can fail after earlier ACKs | Existing SDK publication preflights capabilities and total object/byte budgets. A failure injected at the native receipt leaves a real acknowledged payload prefix; retry repeats exact operation keys. Final success requires receipt and claim ACKs. This is not an atomic batch or permanent custody. |

No unresolved craft finding authorizes production. Story, animation, score and
lyric adaptation are outside this retention adapter's changed output. Real
source/voice authority, natural pauses, language fidelity and listening remain
producing-project gates for M06 and later work.

## Requirement-to-test map

All native fixtures are in `tests/spoken_programme_history.rs` and create only
test-owned temporary stores. No production cache, media, voice or project binding
is read or changed.

| Requirement | Executed case |
| --- | --- |
| Spanish v2 affects one of four editions and stales only its review; old selected subjects are not replaced | `spanish_detail_change_preserves_other_consumed_recipes` |
| Existing publication, injected ACK-prefix failure, stable retry, persisted metadata readback, checkout unavailable, original v1 media/text and stale review rejection | `retained_v1_reopens_and_stale_reviews_refuse` |
| Missing retained runtime or waveform refuses without hydration | Missing-object branches in the retained-v1 case; separate temporary stores contain only explicitly acknowledged fixture objects |
| Rehashed private receipts cannot substitute an edition context, declaration pin or release claim | Three altered-receipt branches in the retained-v1 case; external admission and original SDK context replay remain required |
| Budget/capability refusal before publisher writes and mutated output refusal | `publication_preflights_caps_and_rechecks_mutated_output_before_writes` |
| Mandatory protection has no cooperative fallback or writes | `unsupported_protected_input_refuses` |

The additive `spoken-edition` review target binds compiled/execution/native
recipes, exact runtime, waveform and per-edition access representation. Matching
a target is correspondence only; existing authentication/approval flags remain
false. The SDK source pin remains
`0ceade2a16de1c1ed6cc27e48675e7945d7b61df`; no vendored code changes.

## Verification

Windows, Rust 1.98.0, isolated target `F:/tmp/reel-builds/media-m05-isolated`;
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`.

```powershell
cargo test --locked --features tig-history --test spoken_programme_history --test spoken_programme_media --test spoken_programme --test cue_relative --test audio_event_clocks -j2
cargo test --locked --features tig-history --test spoken_programme_history --test v0213_cli -j2
cargo clippy --all-targets --all-features --locked -j2 -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The merged-parent regression command passed 54 tests, none ignored: four native
history cases (36.61 seconds), 21 media/checker cases (40.24 seconds), 20
declaration/compiler cases and nine clock cases. After adding the three rehashed
receipt mutations, the four native cases passed again (40.58 seconds), alongside
the existing review-decision CLI regression (one case, 0.26 seconds): 55 distinct
passing tests overall. All-target/all-feature Clippy with warnings denied,
formatting, local documentation links and `git diff --check` passed.
Hosted Windows/Linux qualification passed on exact PR head `704cec7`;
the full CI run completed in 22 minutes 23 seconds, with Windows media at
22 minutes 2 seconds. The four-edition source
fixture is the M00 48 kHz stereo PCM24 engineering signal, with distinct EN/ES
sample counts and no acoustic word or listening claim. Runtime identities are
derived from the actual executing test binary and checked against protected
native bytes; they are not placeholder producer hashes.

The final local native test executable was 10,291,200 bytes with SHA-256
`61187213b69b4144e491e92e2863d8c5ca46e41a7c5b0b52b946ca2ea6c5a618`.
This is the fixture's runtime identity, not an installed production binary or a
cross-build byte-identity claim.

Resource bounds are fixed in the [contract](../spoken-edition-history-v1.md).
The caller owns metadata finish/readback, store enrollment and root adoption.
Synchronous time checks do not preempt a blocked operating-system call. No
automatic cache initialization, discovery, hydration, deletion, rollout or
production render is introduced.
