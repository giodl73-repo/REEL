#!/usr/bin/env bash
set -euo pipefail

manifest="${1:-works/0001-ash-vale-last-road-before-winter/manifest.yaml}"
platform="${2:-youtube-demo}"

if [[ ! -f "$manifest" ]]; then
  echo "manifest not found: $manifest" >&2
  exit 2
fi

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg is required. See docs/setup/wsl-ffmpeg.md" >&2
  exit 127
fi

if [[ -f "scripts/validate-manifest.sh" ]]; then
  bash scripts/validate-manifest.sh "$manifest" >/dev/null
fi

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

section_value() {
  local section="$1"
  local id_key="$2"
  local id_value="$3"
  local value_key="$4"
  awk -v section="$section" -v id_key="$id_key" -v id_value="$id_value" -v value_key="$value_key" '
    function clean(line) {
      sub(/^[[:space:]]+(-[[:space:]]*)?[A-Za-z_]+:[[:space:]]*/, "", line)
      gsub(/\r/, "", line)
      gsub(/^"/, "", line)
      gsub(/"$/, "", line)
      return line
    }
    $0 == section ":" { in_section = 1; next }
    in_section && /^[A-Za-z_]+:/ { in_section = 0; next }
    in_section && index($0, "  - " id_key ": ") == 1 {
      in_item = clean($0) == id_value
      next
    }
    in_section && in_item && index($0, "    " value_key ": ") == 1 {
      print clean($0)
      exit
    }
  ' "$manifest"
}

shot_count="$(
  awk '
    /^shots:/ { in_shots = 1; next }
    in_shots && /^[A-Za-z_]+:/ { in_shots = 0; next }
    in_shots && /^  - id: "shot-/ { count++ }
    END { print count + 0 }
  ' "$manifest"
)"

if [[ "$shot_count" -le 0 ]]; then
  echo "no shots found in manifest: $manifest" >&2
  exit 3
fi

work="$(yaml_value work)"
work="${work:-reel-shot-cards}"
target_duration="$(section_value exports id "$platform" duration_seconds)"
target_duration="${target_duration:-$(section_value platforms name "$platform" target_duration_seconds)}"

if [[ -z "$target_duration" ]]; then
  echo "unknown platform or missing target in manifest: $platform" >&2
  exit 4
fi

video_file="renders/shot-cards/${work}-${platform}-shot-cards.mp4"
if [[ ! -s "$video_file" ]]; then
  bash scripts/render-shot-cards.sh "$manifest" "$platform" >/dev/null
fi

out_dir="renders/contact-sheets"
out_file="$out_dir/${work}-${platform}-contact-sheet.png"
mkdir -p "$out_dir"

columns=4
rows="$(((shot_count + columns - 1) / columns))"
fps="${shot_count}/${target_duration}"

ffmpeg -hide_banner -loglevel error -y \
  -i "$video_file" \
  -vf "fps=${fps},scale=320:-1,tile=${columns}x${rows}:padding=8:margin=8:color=0x111111" \
  -frames:v 1 \
  "$out_file"

echo "$out_file"
