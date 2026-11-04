# Native calibrated sample clock

The new sample-clock entry removes the Python scheduler from one concrete music
production step. It captures exact audio, calibration and native MIDI controls,
rejects unsupported external timing, and schedules calibrated source windows
without pitch/time conversion. Existing score parsing/export behavior retains
its original path; the new consumer applies tighter parser budgets and rejects
part-track tempo changes rather than ignoring them.

Synthetic tests verify frame boundaries, stereo samples, quadratic release,
independent overlapping unisons, bounded attenuation and immutable inputs. Bad
pins, missing pitches, insufficient sample holds, invalid output clocks,
external part tempos, NaNs and oversized output refuse before output creation.
The four existing score/export CLI regressions also pass.

A private retained 120-second fixture uses twenty calibrated pitches and 403
notes. The actual candidate produces all pitches with zero onset/release frame
error against its retained ledger. Independent probing confirms 5,292,000 stereo
PCM24 frames at 44.1 kHz; primary hashes remain unchanged. The new 31,752,044-byte
instrumental is an unreviewed technical result, not a selected arrangement or a
claim of equality to an earlier balanced master. Raw project evidence stays
private outside this repository.

Producer-applied music reconstruction, sound, editing and provenance roles
require explicit sample/tick rounding, complete notes, captured primary inputs,
declared gain/release, finite unclipped output and no rights or approval transfer.
No independent reviewer is claimed. Hosted qualification, managed retention,
vocal mixing and consumer adoption remain separate requirements.
