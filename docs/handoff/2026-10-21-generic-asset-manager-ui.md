# Generic asset manager: inventory refresh and offline review

REEL now provides a reusable path from project configuration to a fresh census,
offline block review and existing native dry-run/application commands. Start with
[the workflow](../asset-manager-workflow-v1.md) and
[`manifests/templates/asset-manager`](../../manifests/templates/asset-manager).

`manager-review <project.json> <unique-name> --refresh-inventory` explicitly
collects active-cache metadata and freezes inventory/policy/plan/scope/HTML in a
new private review directory. It preserves original controls and does not change
payloads, ingest reports, inspect archive roots or start a server. Its result
identifies the inventory bytes/hash and flags required private asset retention.
Without the flag, the exact configured registered inventory remains the input.
New project declarations may omit inventory and must explicitly refresh.

The UI adds observed-tier filters, logical/reclaim sorting, chosen-only review,
hold reasons and bounded hash previews. Names and placement roles come from
project configuration. `path_policy: configured-local-roots` permits other
projects' local drive layouts; omitted policy preserves legacy behavior.

Validation and role findings are in
[the review](../reviews/2026-10-21-generic-asset-manager-ui.md). No human approval,
browser visual acceptance, real-project inventory reconciliation or production
runtime adoption is implied. Consumer repositories should adopt a tested source
revision explicitly, retain reports in their own cache/LFS, and generate a fresh
review after registration changes. Do not retarget their existing LFS adapter or
replace running jobs as part of opening a review.
