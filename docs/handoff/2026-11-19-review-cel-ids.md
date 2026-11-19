# Cel IDs on scene review copies

The existing scene-delivery renderer supports an optional `review_ids` policy.
The delivery job binds an exact font and size:

```json
"review_ids": {
  "font": {"path":"fonts/review.ttf", "sha256":"EXACT_SHA256", "bytes":12345},
  "font_size":16
}
```

The scene-delivery compiler profile instead uses a selected scoped binding:
`"review_ids":{"font_binding":"review.font","font_size":16}`. It resolves the
font before writing compilation outputs, adds it to the selected presentation
closure and emits the exact delivery-file reference in the chosen asset-path
mode. Native source preparation retains its guarded input, and scene-build
receipts include it. No font download or implicit system-font selection occurs
in production; fixture code explicitly selects an installed test font.

Labels derive from the compiled cue-relative attachment target, not guessed
filenames or a second hand-authored timing list. Cel targets display `CEL ID`;
shot targets display `SHOT ID`. The retained `review-ids.json` maps each label to
attachment ID, shot/cel identity, exact source FileRef, native sample endpoints
and actual delivery frame endpoints. It also records sample/frame clocks, job
and contract hashes, font/size, and the absence of creative approval.

The overlay applies only to `review.mp4`; master picture/audio composition uses
the existing render path. No policy preserves legacy output names and plan
serialization. Enabling it adds `review-ids.json` and the exact
`review-id-font.ttf` to the output contract. The font is kept in the private
delivery evidence; public packaging and font rights remain producer decisions.

Labels use half-open frame intervals from the actual compiled delivery plan,
including explicit frame allocations, rather than rounded subtitle times.
FFmpeg receives fixed relative evidence/font filenames, restricted file/pipe
protocols and no shell. Text expansion is disabled. IDs are currently restricted
to 1–128 ASCII letters/digits or `_./-`; unsupported characters refuse explicitly.
At most 128 picture labels, 8–96 px font sizes, a 16 MiB font and 1 MiB map are
admitted. Glyph presence and advance width must fit the canvas. Missing/stale
fonts, unsupported label IDs and overflowing labels refuse in planning.

The checker recomputes map correspondence and verifies the exact retained font,
including borrowed source/output providers without physical sibling files.
It also retains existing media decode/clock checks. These checks do not provide
OCR or exhaustive adversarial verification of all encoded label pixels. The
synthetic render fixture separately checks visible labels at the first/last/cut
frames, different label masks across the cut, exact decoded master parity,
borrowed inputs/scattered outputs, and rejection of a changed map even after
repinning its receipt. Refused evidence and the restored valid output are kept.

The initial expanded fixture failed in an older test helper that assumed motion
snapshots existed; static labels now use a dedicated scattered-output setup.
The failed fixture root `F:/src/tig-dev-temp/.tmpVWusRL` remains preserved. The
passing expanded fixture is `F:/src/tig-dev-temp/.tmpl8Mp9c`; the earlier visual
contact check is `F:/src/tig-dev-temp/.tmpqRDvDG/review-ids-contact.png`.
Final source-pinned qualification passes against source
`cfb3319b4c56212eb3dec85a326ba4a0aeff6823` (all 1,850 archived files match).
The frozen render fixture passes in 10.18 seconds; the explicitly enabled legacy
borrowed-input/PCM/pixel regression passes in 10.95 seconds. Strict Clippy,
the Tig library/build CLI feature check and the selected-font profile unit test
pass. The initial legacy harness invocation was skipped by its ignore marker;
only its subsequent explicit `--ignored` run counts as regression evidence.

Evidence:
`F:/src/tig-dev-temp/reel-review-cel-ids-20261119-v1/qualified-tools.json`.
Frozen fixture SHA256:
`31e37dd3a605a6c55b190697ee5e94de5fe1dba32fa47618f08f1376fdca3190`.
FFmpeg/FFprobe/font pins are recorded and reverified after testing. Final root
`F:/src/tig-dev-temp/.tmpNRidyj` retains valid local and borrowed deliveries,
the exact font, original controls and refused tamper evidence. The frozen contact
image `contact.png` in the evidence packet was visually inspected: `CEL red` and
`CEL blue` are legible on their respective sides of the cut. This qualifies the
bounded Windows renderer fixture, not a complete native movie workflow or project
release. Preserve the roots/evidence; no cleanup follows this qualification.

Producer-applied editing/animation checks preserve exact cut boundaries and keep
labels in the review derivative. Sound checks preserve the master and native cue
clock. Rights/provenance checks require the declared selected font and private
evidence. These are producer checklists, not independent review, font permission,
creative approval or publication authority. Workflow review-map to renderer
translation, complete native movie-worker qualification and issue/fix routing
remain open.
