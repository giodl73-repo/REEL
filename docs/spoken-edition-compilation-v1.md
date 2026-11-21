# Spoken-edition control compilation (M02)

**Status:** implementation and fixtures pass local checks on merged M01 `f473431`
(PR #379). Hosted qualification is required before M02 lands. This is control
compilation, not audio production or authority approval.

```sh
reel spoken-edition-compile PRIVATE_EDITION.json --output NEW_PRIVATE_PLAN.json
reel spoken-edition-compile PRIVATE_EDITION.json --profile spoken --output NEW_PRIVATE_SPOKEN_PLAN.json
```

The [declaration](spoken-edition-v1.md) selects modules, language/audience script
variants and performances. Compilation verifies named alignment metadata and
derives exact sample placements. It opens no source audio, picture, consent or
authority payload and invokes no media/provider process.

## Alignment metadata reuses the native word clock

Each selected performance's `alignment` FileRef names exact
`reel.spoken-alignment.v1` bytes. The envelope binds performance/cue/language,
source FileRef, rate/duration and script variant/hash to that declaration.
Its observation is explicitly one of:

| Kind | Required evidence | What it establishes |
| --- | --- | --- |
| `engineering-signal` | Exact synthetic source/clock/marker declarations; no word bindings or observed text; the declared observation hash is SHA-256 of empty bytes | A controlled engineering clock, with no claim of spoken words |
| `word-evidence` | Exact submitted observed text and its declared hash; the existing `reel.scene-word-timing-evidence.v1` type | Metadata identity and complete lexical correspondence against the selected script and observation; no audio/listening or language approval |

Word evidence binds the selected take hash, cue, language, rate and native end
sample. Native words have positive in-range intervals in start order, at most
8 KiB per word and no control characters. Existing `verify_word_text` checks the
lexical sequences; script, submitted observation and recognizer words remain
distinct layers even when lexical normalization considers their text equivalent.

Every declared word/phrase marker has one `word_bindings` entry with `marker_id`,
`first_word_index` and `end_word_index_exclusive`. The interval is nonempty and
in bounds. A word marker selects one native entry; a phrase selects a contiguous
range. Its sample start is the first entry's start and its end is the maximum end
in that range. No missing, extra, duplicate or inconsistent binding is accepted.
Engineering markers remain explicitly synthetic and need no lexical binding.

The public M01 declaration fixture has placeholder pins and cannot be compiled
as though its alignment files existed. M02 tests prepare actual tiny metadata
bytes with matching pins while deliberately supplying no audio. Producing
projects must provide their own admitted evidence; this command invents none.

## One edition, one consumed recipe and clock

| Compiled field | Meaning |
| --- | --- |
| `recipe` / `recipe_sha256` | Selected document/edition identity, module/cue requirements, exact chosen script/performance/evidence, joins, output profile and compiler identity |
| `cues` | Source/placed half-open intervals, speaker role, engineering status and separately retained submitted observed text |
| `chapters` | Module entrance/end on that edition's actual selected clock; an explicitly overlapping join may give adjacent chapters overlapping intervals |
| `word_evidence` | Positioned recognizer word strings and native intervals, explicitly evidence rather than automatically chosen transcript text |
| `markers` | The checked marker intervals shifted by the actual selected source/edition placement |
| `declaration_semantics_sha256` | Canonical typed aggregate declaration identity, retained separately from per-edition consumption |

The first cue starts at zero. Each subsequent start is the previous end plus its
declared added pause or minus its authorized overlap. Recorded silence already
belongs to the selected source interval and is not added again. Trimming must
retain protected regions and every consuming word/marker interval. No stretching,
resampling, language fallback, picture clock or filename-derived ordering occurs.

Unused script/performance variants and global registry ordering are excluded from
an edition's consumed recipe. Set-valued requirements are canonicalized; scope is
projected to the actual consumer while preserving its evidence and role/language.
Expanding an allowlist for a new edition does not invalidate unchanged consumers.
The recipe's performance is a consumed projection, not the original full record;
authored cue/module/join order is preserved. Changing a Spanish detail performance
therefore changes its consuming technical recipe, while the other three retain
their identities under the same compiler. A changed compiler identity is a new
runtime context; historical replay/admission remains M05 work.

## Limits, output and verification boundary

Controls use the declaration's parent directory as their scoped local root for
the CLI. Library callers can supply the existing borrowed `InputFiles` port.
Only the union of selected performance envelopes is read. Unknown provider files
are neither discovered nor treated as selected inputs. An explicitly supplied
metadata map must match that selected key set exactly.

Bounds are 256 KiB per declaration/envelope, 128 selected envelopes, 4 MiB total
alignment metadata, 1,024 words/markers per envelope, 8 MiB derived position/text
metadata, 16 MiB final plan at 48,000 Hz. The explicit `engineering` profile
(CLI default) caps editions at 720,000 samples/15 seconds; `spoken` allows
1,440,000/30 seconds to cover the M00 spoken brief without retiming. Profile
choice enters each consumed recipe and is never inferred from language or
observation kind. Neither profile proves an editorial target duration is met.

M03 must enforce the selected profile's decoded input budget (16 MiB engineering,
32 MiB spoken), 64 MiB total output, 128 MiB workspace and 30-second operation
timeout. Individual declared takes retain M01's 15-second bound. Invalid input
or an exhausted control budget creates no final plan. Publication uses a new
destination with no-clobber atomic staging; it never replaces an existing plan.

The source-bearing plan is private. Normal output is counts and explicit status:
`alignment_metadata_verified: true`, `media_verified: false`,
`authority_verified: false`. Diagnostics use relationship classes and bounded
indices instead of exposing private text/path/voice values. Current compiler bytes
are identified, but that is not native SDK admission or authority to execute.

M03 must rederive/check controls before assembly rather than trust editable JSON
as an executable capability. M04 owns independent media/text checking; M05 owns
retained asset integration; actual wording, naturalness and authority decisions
remain separate gates. See the [M02 design](media-expansion-m02-design.md) and
[test matrix](media-expansion-test-matrix.md).
