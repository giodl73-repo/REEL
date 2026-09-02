# Synthetic same-music language adaptation fixture

These two invented text fragments contain no BERTICA lyrics or translation.
The v0.3.8 integration test copies them into a temporary fixture, generates a
four-second 8 kHz mono raw-PCM accompaniment, and writes the hash-bound
adaptation manifest. Generated audio and absolute local paths are not checked
into git.

The source has four declared units and the target has five. The unequal final
translation link therefore requires an explicit prosody exception. The target
underlay uses the four-note corrected synthetic model and preserves every
governed model target.
