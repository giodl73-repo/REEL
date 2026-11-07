# Preserve a redacted capture budget reason

Native context capture previously reported its bounded stage while hiding all
lower-level errors. A refused freeze therefore looked the same for a resource
budget and a source-contract error, preventing a producer from choosing the
correct next action.

The refusal mapper now recognizes only typed budget errors from the native
source, tree and metadata providers. It retains the existing public stage and
adds a fixed budget message. Unknown failures retain their previous generic
message; arbitrary error text cannot classify a failure. Private paths, IDs and
error-chain text remain hidden. The message does not identify which particular
limit was reached.

Capture, storage, limits, source ownership and execution remain unchanged. This
diagnostic grants no setup, limit amendment, reclamation, render or approval
authority. Producer-applied platform/privacy review requires typed classification
and complete redaction; storage review leaves the original refused operation
and its immutable configuration authoritative.
