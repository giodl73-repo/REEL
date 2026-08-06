# Caption accessibility checks in CLI v0.2.8

`caption-check` gives editors and platform reviewers a deterministic gate before
a caption file enters an expensive render:

```text
reel caption-check candidate-v028.srt --output json
```

The default REEL operational policy requires:

- at most 42 characters on any line;
- at most two lines in a cue;
- at most 20 visible characters per second; and
- at least 1,000 ms of display time.

The command also requires valid UTF-8, positive cue duration, contiguous indexes
beginning at one, and no overlaps. Each policy threshold is configurable with
`--max-chars-per-line`, `--max-lines-per-cue`,
`--max-reading-speed-cps`, and `--min-duration-ms`. These defaults are a
conservative production baseline, not a claim that one policy fits every
language, audience, broadcaster, or accessibility program.

Passing and failing runs emit `reel.caption-check.v0.1`. A failing run prints the
complete report to stdout and exits nonzero. Violations contain only cue index,
rule, measurement, and limit. The report contains the SRT hash and aggregate
measurements but no caption text, filename, or local path. Share it only
intentionally: a hash still binds the report to a particular caption file.

## Boundary

This check evaluates the supplied SRT. It does not OCR a rendered video, prove
that captions were burned into pixels, judge translation accuracy, or replace
human review for wording, line breaks, safe-area placement, contrast, font size,
or device legibility. `animatic-check` separately binds the rendered artifact to
the hashed caption input.

This is additive CLI/report behavior. `reel.manifest.v0.2` is unchanged and no
manifest migration is required.
