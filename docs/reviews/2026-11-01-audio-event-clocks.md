# Explicit audio-event assembly clocks

Entry main `03248570b5f3dd2671a2c93459ebbae15cf816ca`;
WP-010/011/012, REQ-006/009/010, SPEC-010/011/012, IF-005/010/011,
PKG-004/006. Scope: an explicit clock-source schema in the existing integer-sample
compiler, synthetic fixtures and teaching docs. No new renderer, cache operation,
source selection, approval, runtime installation or production job.

The narration contract remains unchanged. A separately tagged audio-event
contract binds ordered clock IDs to production audio events. Missing or duplicate
event IDs refuse; narration IDs cannot substitute for absent events. Word/phrase
markers refuse in this nonspoken clock domain. Targets and sample/frame arithmetic
reuse the existing compiler. The compiled schema also differs, keeping clock
domain part of the exact compiled identity even when ID strings coincide.

Existing public Rust structs and old serialized output remain unchanged. L0:
format, scoped warnings-denied Clippy and diff. L1: domain separation, missing and
duplicate event IDs, marker refusal, retained frame-window and duration boundaries.
L2: ordinary CLI and delivery planning, plus unchanged legacy output compared
against a preserved pre-change executable. Hosted qualification and actual
project/producer adoption remain separate.

Producer sound/editing lenses require native music/recording clocks without
invented speech or inferred old-timestamp scaling. Privacy/rights keeps declared
time, measured audio, source eligibility, producer correspondence and creative
approval distinct. The running component verifier remains pinned to its immutable
installed executable and frozen source; development changes no live profile.

Local verification: 37 scoped regular tests pass (five domain/clock, four legacy
cue and 28 delivery cases), plus one explicit native render/check fixture. The
fixture uses nonzero synthetic recordings with no narration and preserves every
signed source sample across the music-stem join. An initial fixture incorrectly
carried the dialogue-only `cue_id` onto the music bus; the existing guard correctly
refused. The corrected fixture uses normal music attachments and keeps a negative
case for that guard. No library guard was weakened.

The actual CLI compiles an audio-event contract. Both original/recast legacy
conform files are byte-identical to output from a separately preserved pre-change
executable built at the entry source. Strict all-target/all-feature Clippy passes.
Raw baselines, runtimes and logs remain private outside Git. Hosted qualification,
qualified installation and complete private-video producer adoption remain open;
this does not convert the held video source or authenticate an old approval.
These are producer-applied checklists, not independent reviewers or approval.
