# Recover an independently declared PCM source

A lossless carrier can retain the exact PCM of a missing primary recording.
Recovery is useful only when the reconstructed WAV matches an independently
known complete-file hash and length. A similar compressed recording is not an
acceptable replacement.

`reel-tig-job recover-pcm-wave PROFILE.json` streams a canonical PCM WAV into a
verifier. It creates no payload file or cache entry. The profile declares the
existing guarded carrier, expected target identity, PCM shape, byte budget,
deadline and exact runtime bindings. Only integer PCM with a canonical 44-byte
header is supported. Container metadata or other WAV layouts are not inferred.

`--publish` additionally requires explicit admission to an existing publisher.
It streams into that publisher, verifies the complete target, and reopens the
published object. A retry can reuse verified bytes without the carrier. A new
operation can reuse the same verified target; corrupt existing bytes are refused.
No new cache, intermediate movie or temporary decoded WAV is created.

The decoder owns its child process. Its watchdog kills only that owned child;
output length, SHA-256, exit status and bounded stderr must pass. Source and
runtime guards remain held during decoding. Existing per-object publication
coordination is used without a shared render lock.

Producer-applied music reconstruction, sound, editing and provenance checklists
require exact original identity and duration, immutable production receipts,
private operational profiles and separation of recovery from creative selection.
This is not independent review. Recovery neither changes the selected asset nor
asserts original producer identity, independent custody, approval or eviction
authority. A project must separately revise its selected primary record and
verify consumers against that committed revision.

The synthetic fixture removes the original WAV before recovery, proves dry-run
non-publication, publishes exact bytes, retries after removing the carrier,
reuses through another operation and refuses corrupt destination bytes. Negative
cases cover wrong hash, shape, budget, target, timeout and unknown fields. A
separate watchdog test checks termination of its owned sleeping process.
