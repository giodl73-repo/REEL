# REEL semantic dependency workbench

Status: scoped diagnostic client. Dated pulses and build provenance retain qualification; native production acceptance remains separate.

Reader task: open an owned reel.semantic-assembly.v1 graph, choose a node or
presentation, optionally supply a selected pointer, and inspect the exact native
dependency closure, selected assets, semantic events and declared/computed locks
without installing a native toolchain. Use original reel-assembly types/functions;
no JavaScript graph algorithm or competing contract. A synthetic authored graph
supplies the default. No project media or private production records are bundled.

## Source and preservation

Public entry e4d22e0 (REEL 0.3.22) is 271 public commits beyond the older common
ancestor with TRACKER's canonical 749270f. Merge fe08024 preserves the canonical
PITFALL addition without reverting current production code, root Cargo files,
crates or vendor. The colliding historical pulse-06 filename retains the current
record; the old PITFALL pulse is copied verbatim to
`context/waves/2026-08-04-bertica-production-hardening/pulses/pitfall-doctrine-2026-08-30.md`.
The original commit remains on remote pitfall/production-doctrine-20261102.
Existing owner worktree reel-worktrees/audio-repair is outside this work.

## Scope and gates

- Preserve src/, crates/, vendor/, root Cargo.toml/lock and native workflows.
- Expose validate_graph, closure, selected_closure, presentation_closure and
  graph_lock. Preserve native ordering, error text and field meanings.
- Native graph validation does not establish whole-graph acyclicity or cache
  availability. Closure checks cycles reachable from its selected node and omits
  unselected slots. Keep their dispositions visible; success is not readiness.
- Selected pointers compare supplied lock identity. Compare the separately
  computed graph_lock as an explicitly labelled observation; do not rewrite the
  graph or silently promote declared hashes to verified source/asset bytes.
- No media reads, restoration, TIG calls, providers, renderer execution, creative
  scoring, human consent, upstream canon changes or publication authority.
- Downloads of user diagnostics contain declared identities and are local
  working reports, never privacy-safe release receipts.

## Entry role review

Agent review using repository checklists, not human approval:

| Lens | Finding and disposition |
|---|---|
| Editor | Native dependency scope helps inspect scene/presentation handoff; it does not judge rhythm or creative selection. Preserve omissions and lineage rather than infer complete edit readiness. |
| Sound Designer | Show explicit-silence and unselected dispositions separately; neither proves an actual recorded track or a successful mix. Do not synthesize sound. |
| Platform/Audience | Provide own-file input, keyboard/mobile use, bounded worker execution, original/current/request/report downloads and independent source/legal routes. No native browser renderer replacement. |
| Rights/Provenance | Use wholly authored synthetic metadata; never fetch caller cache URIs. Separate supplied hashes, computed graph identity, pointer declarations and actual media verification. Reports may contain user identities; no approval or shareability claim. |

## Pulses and validation

1. Preserve current production/PITFALL history; test and WASM-check unchanged
   reel-assembly with locked root dependencies and strict lint.
2. Add a standalone unpublished WASM/native adapter with native types, explicit
   input/graph/output budgets and exact independent native controls. Preserve
   production dependency versions from the root lock for hash semantics.
3. Add browser authoring/recovery, native/WASM parity, zero/invalid/missing/
   unselected/cyclic/pointer-mismatch coverage and real resource/deadline checks.
4. Package complete source/legal notices, independently replay outside Git,
   qualify hosted exact head, merge, verify main/live and only then snapshot
   TRACKER. Recheck current public main before integration; other sessions may
   advance production code. Preserve all owner work/data and shared Cargo cache.
