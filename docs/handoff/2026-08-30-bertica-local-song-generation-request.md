# BERTICA handoff — local exact-lyric song generation

Date: 2026-08-30  
Consumer: private adaptation of *El camino de los caimitos*  
Requested engine boundary: ACE-Step 1.5, installed and run locally

## Production need

BERTICA has an approved instrumental main-title proof and needs an original
Spanish song audition built from one poem in the authorized manuscript. The
poem wording must remain exact. The audition should support a Cuban-rooted
composition direction while avoiding imitation of any named artist and avoiding
use of a family member's voice identity.

## Reusable REEL requirements

- Bind exact lyric bytes and source ranges by SHA-256.
- Keep engine prompts provider-neutral and named-artist imitation disabled.
- Declare the local engine, model, pinned revision, license, seed, and parameters.
- Reject third-party upload and non-local generation for private inputs.
- Require recorded speaker-specific consent evidence for any assigned voice;
  allow an original unassigned singer without identity consent.
- Bind optional reference audio/MIDI by hash and local-only egress policy.
- Request full mix, instrumental, vocal, or stems as distinct outputs.
- Write a private request and a shareable path-free, lyric-free receipt.
- Diagnose readiness without downloading or invoking the engine.
- Require human listening and a separate public-release decision.
- Never claim generated lyric fidelity merely because exact text was supplied.

## Implemented in v0.2.25

The `song-validate`, `song-engine-plan`, `song-engine-plan-check`, and
`song-engine-doctor` commands implement that boundary. Actual ACE-Step execution
and generated-audio lyric verification remain deliberately outside this first
contract and can be added as separate, receipt-bound steps.
