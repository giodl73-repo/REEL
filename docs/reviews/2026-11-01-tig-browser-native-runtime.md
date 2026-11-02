# Native browser runtime integration

The candidate [native runtime](../tig-browser-native-runtime.md) connects the
bounded capture interface to an owned Chromium-compatible browser and a native
FFmpeg encoder without source copies or PNG spooling. It reuses the shared
Windows process runner with atomic Job Object assignment, explicit handle
inheritance, quiet output, memory/process limits and descendant teardown.
The initial native contract is FFV1/yuv444p plus optional 48kHz/stereo PCM24.

Actual synthetic browser runs exposed four gaps absent from metadata fixtures:
HTTP discovery must complete at Content-Length without EOF; CDP wire timestamps
can be fractional; browser tab icons are non-rendered auxiliary requests; and
command IDs must fit Chromium's signed integer range. The fixes keep asset JSON
identity rules strict, deny only the browser-owned icon request, and check owned
shutdown rather than waiting out an unrecognized command. The whole browser
working/TEMP directory is owned, so auxiliary files are retired with the profile.

The real six-frame SVG fixture passes native format/count checks, independently
confirms changed decoded pictures, and verifies every decoded PCM byte against
12,000 asymmetric nonzero samples per channel. A corrupt-source fixture asserts
the exact resource hash refusal, preserves an unrelated output and leaves no
profile or partial media. Source/program pins remain held through child teardown.
These are synthetic fixtures, not private artifact or retained-master acceptance.
The real SVG also consumes a declared UTF-8 image path with spaces, percent,
plus and ampersand characters; exact served-byte totals confirm resolution.

Shared process fixtures exercise actual Windows memory/process limit, preflight,
timeout and cancellation checks, including an otherwise orphaned descendant.
Two loopback discovery fixtures cover response framing/refusals. Capture tests
also distinguish finite fractional wire values from identity metadata and reject
duplicate keys; an authored Image/Document request gets no tab-icon exemption.
Local checks pass all 28 shared runner lifecycle fixtures, 27 browser units,
both explicitly executed native browser fixtures, twelve source-planning/CLI
regressions, strict all-feature/all-target Clippy, formatting and whitespace.
The minimal Tig-feature library/scene-CLI check passes separately. The memory
fixture uses a completing control and allocation markers instead of assuming
immediate exit under Windows quota pressure. Hosted checks must qualify the
final exact source head, including the real fixtures now added to Windows CI.

Producer-applied Editor and Rights/Provenance checklists require unchanged source
timing, independent audio/picture checks, immutable approval subjects, explicit
program enrollment and no asset/network fallback. Storage/recovery checks require
bounded frames, owned paths, no unbounded logs, no-clobber output and stopped owned
children. Workspace accounting is observational rather than a filesystem quota;
child memory limits do not claim a whole-host quota or network sandbox. This is
producer review, not independent approval or complete environment qualification.

Managed source/job enrollment, frozen-source production replay, native produced
claims, CAS retention, record/Git handoff, complete per-packet clock and retained
reference checks, private acceptance and lane entry/rollback remain open. Existing
tools, historical routing, selections and active bindings are unchanged.
