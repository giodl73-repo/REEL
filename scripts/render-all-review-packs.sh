#!/usr/bin/env bash
set -euo pipefail

root="${1:-works}"

if [[ ! -d "$root" ]]; then
  echo "works root not found: $root" >&2
  exit 2
fi

mapfile -t manifests < <(find "$root" -mindepth 2 -maxdepth 2 -name manifest.yaml | sort)

if [[ "${#manifests[@]}" -eq 0 ]]; then
  echo "no work manifests found under: $root" >&2
  exit 3
fi

out_dir="renders/review-packs"
index_file="$out_dir/INDEX.md"
mkdir -p "$out_dir"

{
  printf '# REEL review-pack index\n\n'
  printf '%s\n' "- Works root: \`$root\`"
  printf '%s\n\n' "- Generated: \`$(date -u '+%Y-%m-%dT%H:%M:%SZ')\`"
  printf '| Work manifest | Review pack |\n'
  printf '|---|---|\n'
} > "$index_file"

for manifest in "${manifests[@]}"; do
  report="$(bash scripts/render-review-pack.sh "$manifest" < /dev/null)"
  printf '| `%s` | `%s` |\n' "$manifest" "$report" >> "$index_file"
done

echo "$index_file"
