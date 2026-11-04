# Native vocal and instrumental stems

An optional exact vocal source extends the sample-clock request. Its rate,
channels and complete frame count must match; the only vocal transform is the
declared scalar gain and common attenuation. There is no hidden trim, padding,
resampling, pitch processing or vocal generation. The supplied vocal estimate
retains its original attribution, limitations and rights scope.

The PCM24 master is constructed from the quantized instrumental and vocal stems,
so integer stem-sum equality is exact. Common headroom protects each stem and
their sum, including quantization rounding. Large bank buffers are released
before loading the vocal. Invalid voice controls and clocks refuse before any
output directory is created.

Five unit tests cover the existing sample-clock behavior plus complete stereo
stem sums, unchanged source-clock boundaries, attenuation and voice refusals.
The read-only checker also rejects self-consistent replacement hashes when the
master sum is wrong or when both the vocal and master are changed together.

The actual private 120-second fixture produces three 31,752,044-byte WAV files.
All output pins and clocks pass; every vocal sample matches the unchanged
source under the declared gain, and the master equals the exact PCM24 stem sum.
The mix needs no common attenuation and reuses the previous instrumental bytes.
It remains an unreviewed technical fixture, not a newly selected arrangement.

Music reconstruction, sound, editing and provenance checklists preserve primary
identities, complete frame counts, explicit gains, exact stem sums and separate
approval. No independent reviewer, cleaner vocal estimate, custody, delivery or
managed producer adoption is claimed. Hosted qualification and normal-entry
adoption remain required before use as a production binding.
