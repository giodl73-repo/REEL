# Optional editable text contrast

A consumer's source-bound year label lost contrast over bright surf. The
generic Text style can now opt into an outline and shadow through template
data. Existing templates omit `text_effects` and retain identical ASS bytes.
The source, semantic marker, event positions, durations and chapter styles
are unchanged.

Verification: `cargo test --test template_presentation` passed all 20 tests.
The regression checks omitted-field serialization and compilation, style-only
differences, native clocks and rejection of negative/nonfinite dimensions.
`git diff --check` passed.

Independent static review by bounded agent
`/root/s2e03_sc003_corrected_pixel_review` found no blocking defects under the
editor and platform/audience lenses. Report SHA-256:
`e8d8257a74ff5b5c81cb27661f5885df4e990295def83bbf3ca5018cf6d544d0`
(5,694 bytes; consumer cache). This is a code review, not human visual approval.
Actual consumer typography still needs a rendered-frame check.

JSON manifests are backward compatible. Rust callers constructing the public
`EditableTextTemplate` struct directly must add `text_effects: None`.
