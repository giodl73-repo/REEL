# Editable packet carriers for bounded publication jobs

Loose parts exposed a real admission problem: a two-page issue could admit 119
source objects but could not add its result while preserving the native
128-path close guard. Both affected attempts remain protected and incomplete.
Their controls, original WorkRefs, activation, pending result and scratch are
preserved. The guard was not raised or bypassed.

New comic requests can opt into `editable_packet: true`. The producer returns
`editable.packet` as a named output with media type
`application/vnd.reel.editable-packet.v1`. It contains the complete rendered part
and captured editable sources. The technical receipt is separate, avoiding a
receipt/carrier hash cycle. Part-scoped output IDs remain unchanged.

Issue requests can declare `bundles`, a part-ID-to-pin map, alongside their
existing `parts` receipt pins. Each bundle path is its receipt's sibling
`editable.packet`. Native inputs bind each exact retained carrier and receipt;
the full parent result is verified before child admission. The child owns the
actual complete carrier bytes, rather than declaring every embedded file as a
separate native object. This is a publication format over existing Tig storage,
not another cache or protection engine.

The reader checks the whole carrier pin, canonical index, file hashes, lengths,
file/directory and case aliases, reserved device names and portable paths. Limits
are 256 files, a 1 MiB index and 128 MiB total carrier bytes. There is no
compression, extraction, symlink metadata or code execution. Existing stricter
native input/output, transition and staging limits still apply.

Assembly deterministically replays each complete part from the bytes in its
carrier and checks the separately pinned receipt. The resulting private issue
still contains all unpacked editable parts, their carriers, reading derivatives
and relocated controls. Filesystem assembly accepts exact packed-only or full
part directories; present unpacked copies must also match. Missing or corrupt
carriers refuse; there is no loose-file or latest-version fallback.

Existing requests omit the new fields and keep their original bytes/identities.
Loose-file assembly remains supported within its actual resource bounds.
Technical checks do not approve font rights, canon, translation, print or release.

All 70 shared tests and strict shared Clippy passed, including canonical framing,
malformed/corrupt carrier refusal, same-byte logical-origin refusal and complete
editable issue replay. Native end-to-end qualification is running in
`F:/src/tig-dev-temp/reel-workflow-native-issue-20261118-v4`.
The native fixture admitted the packed child with 61 resource objects, compared
with 119 for the loose representation. All 37 default native tests and strict
native Clippy passed. The original-operation refusal fixture also passed: the
expected close-budget error leaves the index unchanged, original input claims
reopenable, scratch preserved and the journal in `ImportPending`. Neither loose
attempt is reported as completed. Packed task completion and history/export
checks, plus the final ten native regression cases, remain in progress.
