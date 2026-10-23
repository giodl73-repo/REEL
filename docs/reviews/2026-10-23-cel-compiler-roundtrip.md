# Native cel compiler round-trip review

Base: `427a945`. Scope: shared Rust candidate application, actual delivery
compiler integration tests, and rate-only editor notices. Producer-applied
Editor, Animation Director and Rights/Provenance checklists; no independent or
human creative approval is implied.

| Finding | Correction and actual evidence |
| --- | --- |
| The editor's private CLI function could only be tested separately from the delivery compiler | Moved the existing load/apply implementation into `cel_source::editor`. The CLI delegates to it. A root integration test calls that same API, reopens the candidate, and passes its controls to the actual `scene_delivery_compile::compile_to_dir`. Synthetic declarations are shared with CLI fixtures instead of copied. The root's asset dependency is test-only. |
| Cue-local editing needs proof through delivery compilation | A native two-cue, 48 kHz fixture applies reorder, off-frame holds, selected-picture substitution, removal and comments. Compilation produces production, contract, job, semantic-delivery, build and receipt controls with exact 96,000/88,000-sample cue durations and a 31,000-sample cut. Picture revision ancestry survives, source bytes remain unchanged, and recompile refuses existing output without changing it. No media is read or rendered. |
| Mixed-rate authoring is broader than the delivery compiler | A 48/44.1 kHz candidate remains valid native editable authoring. The real compiler rejects the mismatched delivery profile before creating delivery output, without resampling or mutating controls. The editor preserves this distinction. |
| A successful draft could obscure a known rate mismatch | Sessions expose `delivery_clock`: native rates, optional profile rate, and a rate-only state. A persistent editor notice covers mixed rates and profile mismatch while allowing draft authoring. Matching/undeclared rates make no compiler or readiness claim. Logic and exported-handler tests cover the notice. |
| Compiler success is not visual or media acceptance | Browser automation did not return; the alternate UI inventory reported no connected browsers. No layout/screenshot or audio/video acceptance is claimed. Exported-handler tests remain synthetic DOM execution. Private compatibility results from the previous increment remain metadata-only; no new private compiler or render result is claimed. |

Validation on Windows: 288 asset/assembly Rust tests plus two root compiler
integration tests, strict Clippy for asset/assembly all-targets and the root
compiler integration target, formatting, ROLES and diff checks. All 30 asset/editor
JavaScript tests pass, including 16 cel tests. The compiler integration test is
included by existing root CI test discovery; no renderer is invoked by it.

The native compiler proof is for the synthetic ordinary scene and delivery
controls above. The fixture contains no valid media codecs and certifies no
cache custody or rendered outcome. Full private project compilation, real media
validation/rendering, browser visual acceptance, bilingual authoring and installed
runtime adoption remain separate. No live cache access/movement, server or daemon
startup, private-media egress, golden selection or CAMINOS changes occurred.
