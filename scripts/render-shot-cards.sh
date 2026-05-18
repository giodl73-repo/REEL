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

work="$(yaml_value work)"
title="$(yaml_value title)"
format="$(yaml_value format)"
style="$(yaml_value style)"
platform_aspect="$(section_value platforms name "$platform" aspect_ratio)"
platform_duration="$(section_value platforms name "$platform" target_duration_seconds)"
export_duration="$(section_value exports id "$platform" duration_seconds)"

work="${work:-reel-shot-cards}"
title="${title:-REEL Shot Cards}"
format="${format:-unknown-format}"
style="${style:-unknown-style}"
target_duration="${export_duration:-$platform_duration}"

if [[ -z "$platform_aspect" || -z "$target_duration" ]]; then
  echo "unknown platform or missing target in manifest: $platform" >&2
  exit 4
fi

case "$platform_aspect" in
  "16:9")
    width=1280
    height=720
    font_size=32
    wrap_width=56
    ;;
  "9:16")
    width=720
    height=1280
    font_size=28
    wrap_width=34
    ;;
  *)
    echo "unsupported aspect ratio for $platform: $platform_aspect" >&2
    exit 4
    ;;
esac

out_dir="renders/shot-cards"
out_file="$out_dir/${work}-${platform}-shot-cards.mp4"
mkdir -p "$out_dir"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

shots_tsv="$tmp_dir/shots.tsv"

awk '
function clean(line) {
  sub(/^[[:space:]]+[A-Za-z_]+:[[:space:]]*/, "", line)
  gsub(/\r/, "", line)
  gsub(/^"/, "", line)
  gsub(/"$/, "", line)
  return line
}
function emit() {
  if (id != "") {
    print id "\t" duration "\t" caption "\t" camera "\t" action "\t" narration
  }
}
/^shots:/ { in_shots = 1; next }
in_shots && /^[A-Za-z_]+:/ {
  emit()
  id = ""
  in_shots = 0
  next
}
in_shots && /^  - id: "shot-/ {
  emit()
  id = clean($0)
  duration = "4"
  caption = ""
  camera = ""
  action = ""
  narration = ""
  next
}
in_shots && id != "" && /^    duration_seconds:/ { duration = clean($0); next }
in_shots && id != "" && /^    camera:/ { camera = clean($0); next }
in_shots && id != "" && /^    action:/ { action = clean($0); next }
in_shots && id != "" && /^      narration:/ { narration = clean($0); next }
in_shots && id != "" && /^      text:/ { caption = clean($0); next }
END { emit() }
' "$manifest" > "$shots_tsv"

if [[ ! -s "$shots_tsv" ]]; then
  echo "no shots found in manifest: $manifest" >&2
  exit 3
fi

base_duration="$(awk -F '\t' '{ total += $2 } END { printf "%.3f", total }' "$shots_tsv")"
duration_scale="$(awk -v target="$target_duration" -v base="$base_duration" 'BEGIN {
  if (base <= 0 || target <= 0) {
    exit 1
  }
  printf "%.6f", target / base
}')"

wrap() {
  printf '%s' "$1" | fold -s -w "$wrap_width"
}

concat_list="$tmp_dir/concat.txt"
: > "$concat_list"

index=0
while IFS=$'\t' read -r shot_id duration caption camera action narration; do
  index=$((index + 1))
  card_text="$tmp_dir/card-$index.txt"
  clip_file="$tmp_dir/shot-$index.mp4"
  duration="${duration:-4}"
  duration="$(awk -v d="$duration" -v s="$duration_scale" 'BEGIN { printf "%.3f", d * s }')"

  {
    printf '%s\n' "$title"
    printf '%s | %s | %s | %s | %s | target %ss\n\n' "$shot_id" "$format" "$style" "$platform" "$platform_aspect" "$target_duration"
    printf 'Caption: %s\n' "$(wrap "${caption:-No caption}")"
    printf '\nCamera: %s\n' "$(wrap "${camera:-No camera note}")"
    printf '\nAction: %s\n' "$(wrap "${action:-No action note}")"
    printf '\nNarration: %s\n' "$(wrap "${narration:-No narration}")"
  } > "$card_text"

  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=0x041E42:s=${width}x${height}:d=${duration}:r=24" \
    -vf "drawtext=textfile=$card_text:fontcolor=white:fontsize=${font_size}:line_spacing=10:x=70:y=70,format=yuv420p" \
    -c:v libx264 \
    -pix_fmt yuv420p \
    -t "$duration" \
    "$clip_file"

  printf "file '%s'\n" "$clip_file" >> "$concat_list"
done < "$shots_tsv"

ffmpeg -hide_banner -loglevel error -y \
  -f concat -safe 0 -i "$concat_list" \
  -fflags +genpts \
  -c:v libx264 \
  -pix_fmt yuv420p \
  -avoid_negative_ts make_zero \
  "$out_file"

echo "$out_file"
