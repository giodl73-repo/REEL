# Contributing to REEL

Start with the [architecture](architecture.md) and the exact contract affected by
the change. Keep project content, cache roots, machine profiles and creative
decisions in the producing project. Reuse the native schema/compiler rather than
introducing a second scene, song or asset grammar for one lane.

## Choose the owner of a change

| Change | Owner |
| --- | --- |
| Generic asset/byte identity, Git snapshots, storage lifecycle or catalogue | [Tig upstream](https://github.com/giodl73-repo/tig) |
| Media meaning, compilation, rendering, independent checks or media adapter | REEL |
| Manuscript, selected recordings/cels, project definitions and delivery policy | Producing project |
| External renderer/provider implementation | Its runtime/adapter, with REEL's explicit boundary |

Vendored Tig files must remain exact upstream bytes. Fix generic behavior
upstream, update the [source pin](../vendor/README.md) deliberately, and qualify
the consumer. Do not locally patch a vendored crate or normalize its bytes.

## Validate the change

For Rust changes, use the affected crate/feature tests and a fixture that checks
observable behavior, stale-input refusal and relevant failure recovery. For a
media change, check actual decoded frames/samples using explicit runtime inputs;
an absent-runtime skip is not media qualification.

The repository-wide checks include:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
git diff --check
```

Workspace crates have separate tests (`cargo test -p reel-assembly --all-targets --locked`,
and corresponding commands for `reel-music`, `reel-print`, `reel-assets`). Browser
and real-media fixtures have additional requirements in
[CI](../.github/workflows/ci.yml). The
[WASM workflow](../.github/workflows/pages.yml) independently checks, packages and
rebuilds the delivered semantic workbench.

For documentation-only edits, verify local links/anchors, schema and feature
names, executable arguments and the smallest relevant runnable examples.
Repository-required hosted checks still apply to the PR; a docs edit does not
authorize bypassing them. No new test harness is needed merely to mirror prose.

## Review and land

Apply the relevant [role checklists](../.roles/ROLE.md) and
[pitfalls](../.pitfall/PITFALL.md). Record findings as technical checklist review;
do not invent independent reviewers, creative approval or consent. Keep rights,
selection, listening/visual review, technical verification and release distinct.

Public evidence uses generated fixture data. Keep private paths, lyrics,
prompts, capability URLs, media identities and raw operational reports out of
public issues and CI output. Software licensing and original-content licensing
are different; see [LICENSE](../LICENSE).

Merge after the required checks pass for the current PR state. A source merge
does not install a shared executable, migrate a project's records or replace an
admitted producer. Document those adoption steps separately when requested.
