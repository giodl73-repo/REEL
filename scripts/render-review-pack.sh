#!/usr/bin/env bash
set -euo pipefail

manifest="${1:-works/0001-ash-vale-last-road-before-winter/manifest.yaml}"

if [[ ! -f "$manifest" ]]; then
  echo "manifest not found: $manifest" >&2
  exit 2
fi

if ! command -v ffprobe >/dev/null 2>&1; then
  echo "ffprobe is required. See docs/setup/wsl-ffmpeg.md" >&2
  exit 127
fi

bash scripts/validate-manifest.sh "$manifest" >/dev/null

yaml_value() {
  local key="$1"
  awk -v key="$key" '
    index($0, key ": ") == 1 {
      value = substr($0, length(key) + 3)
      gsub(/\r/, "", value)
      gsub(/^"/, "", value)
      gsub(/"$/, "", value)
      print value
      exit
    }
  ' "$manifest"
}

platforms="$(
  awk '
    function clean(line) {
      sub(/^[[:space:]]+-[[:space:]]*name:[[:space:]]*/, "", line)
      gsub(/\r/, "", line)
      gsub(/^"/, "", line)
      gsub(/"$/, "", line)
      return line
    }
    /^platforms:/ { in_platforms = 1; next }
    in_platforms && /^[A-Za-z_]+:/ { in_platforms = 0; next }
    in_platforms && /^  - name:/ { print clean($0) }
  ' "$manifest"
)"

if [[ -z "$platforms" ]]; then
  echo "no platforms found in manifest: $manifest" >&2
  exit 3
fi

work="$(yaml_value work)"
title="$(yaml_value title)"
work="${work:-reel-review-pack}"
title="${title:-REEL Review Pack}"

out_dir="renders/review-packs"
report="$out_dir/${work}-review-pack.md"
mkdir -p "$out_dir"

{
  printf '# Review pack: %s\n\n' "$title"
  printf '%s\n' "- Manifest: \`$manifest\`"
  printf '%s\n' "- Work: \`$work\`"
  printf '%s\n\n' "- Generated: \`$(date -u '+%Y-%m-%dT%H:%M:%SZ')\`"
  printf '| Platform | MP4 | Duration | Contact sheet |\n'
  printf '|---|---|---:|---|\n'
} > "$report"

while IFS= read -r platform; do
  [[ -z "$platform" ]] && continue

  video_file="$(bash scripts/render-shot-cards.sh "$manifest" "$platform" < /dev/null)"
  sheet_file="$(bash scripts/render-contact-sheet.sh "$manifest" "$platform" < /dev/null)"
  duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$video_file")"

  printf '| `%s` | `%s` | `%ss` | `%s` |\n' "$platform" "$video_file" "$duration" "$sheet_file" >> "$report"
done <<< "$platforms"

echo "$report"
