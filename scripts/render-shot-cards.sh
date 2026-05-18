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

work="$(yaml_value work)"
title="$(yaml_value title)"
format="$(yaml_value format)"
style="$(yaml_value style)"

work="${work:-reel-shot-cards}"
title="${title:-REEL Shot Cards}"
format="${format:-unknown-format}"
style="${style:-unknown-style}"

case "$platform" in
  youtube-demo)
    width=1280
    height=720
    font_size=32
    wrap_width=56
    duration_scale=1
    ;;
  iphone-social)
    width=720
    height=1280
    font_size=28
    wrap_width=34
    duration_scale=0.75
    ;;
  *)
    echo "unknown platform: $platform (expected youtube-demo or iphone-social)" >&2
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
    printf '%s | %s | %s | %s\n\n' "$shot_id" "$format" "$style" "$platform"
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
