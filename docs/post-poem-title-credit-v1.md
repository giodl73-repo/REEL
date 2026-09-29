# Editable title and poet credit after a poem

An `opening-poem` editable text template may set
`post_poem_title_duration_ms` to a positive whole-centisecond interval from
10 ms through 30 s. The cue-relative contract declares the matching
`tail_duration_samples` and extends the selected picture and full-scene ASS
attachment through that interval. Dialogue ends with the last spoken cue;
the tail is explicitly nonspoken. The native poem cue clock remains unchanged. Poem lines
and panel stop at the last native cue sample; the editable title and poet
credit begin at that sample and end after the selected interval. The template
owns the interval. A scene build must provide picture and sound through the
resulting full duration. With this field absent, existing first-frame title
behavior is unchanged. Chapter templates reject the field.

`reel-scene-template compile` requires
`reel.scene-presentation-source-text.v2` for this route. It is distinct from
the older `reel.presentation-source-text.v1` used by legacy scene presentation
and episode display evidence. The V2 source-text file is selected through a
scoped asset binding and records exactly two display units, in order:
`poem-title`, then `poet-credit`. Each unit has a source scope ID, original
UTF-8 text and SHA-256, and editable UTF-8 text and SHA-256. The compiler
checks those against the scene's ordered `presentation_source_scope_ids`, the
selected invocation title and byline, and the language-local poem text and
native cue alignment. This lets an author-approved display correction retain
the unchanged manuscript text and its hash.

The selected scene build checks that the template receipt and cue-relative
scene clock have the same duration. A synthetic full scene build verifies the
native cue ends before the tail, the clean picture covers it, and the selected
master's tail frame differs from the clean frame because of the editable ASS
layer. Title and credit share one tail card, with their source IDs in reading
order. A different timed sequence would require another selected template.

Source IDs and hashes prove the selected input's identity; they do not certify
its human approval or prove pixels in a final scene master. A selected clean
picture, full-duration audio, scene delivery receipt, decoded frame review,
and editorial and author review remain necessary before episode conform.
Poem scenes with additional embedded chapter or section headings need a
separate presentation route for those headings. This title/credit route does
not silently consume or display those extra IDs.
