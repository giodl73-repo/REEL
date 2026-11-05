# Ephemeral browser directory accounting

Windows hosted validation at `2da2bb0f` passed three native browser fixtures but
lost the fourth control connection while receiving the first `Page.enable`
response. The earlier run at `f47673d5` reported a missing-path error. The exact
PCM-window fixture passes locally in isolation. These observations identify an
intermittent failure; they do not prove that every connection reset shares one
cause.

Inspection found a concrete accounting race: parent enumeration can identify a
directory that Chromium removes before the recursive `read_dir`. That error
could cancel an otherwise running owned child. The walker now accepts only a
missing observed descendant. Root absence, other I/O errors, reparse checks and
all existing depth, file-count and byte bounds remain enforced. There is no
process, capture or output retry.

The regression fixture deterministically observes and removes a child before
descent, then checks that accumulated accounting is preserved, a missing root
still refuses and remaining file bytes still enforce the budget. Review is
producer-applied; hosted qualification and production adoption remain separate.
