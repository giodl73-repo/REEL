#!/usr/bin/env bash
set -euo pipefail

manifest="${1:-manifests/templates/scenario-video.yaml}"

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
  awk -F': ' -v key="$key" '
    $1 == key {
      value = $2
      gsub(/\r/, "", value)
      gsub(/^"/, "", value)
      gsub(/"$/, "", value)
      print value
      exit
    }
  ' "$manifest"
}

indented_yaml_value() {
  local key="$1"
  awk -F': ' -v key="$key" '
    $1 ~ "^[[:space:]]+" key "$" {
      value = $2
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
repo="$(indented_yaml_value repo)"
scenario_id="$(indented_yaml_value id)"
caption="$(indented_yaml_value text)"

work="${work:-reel-smoke}"
title="${title:-REEL Smoke Render}"
format="${format:-unknown-format}"
style="${style:-unknown-style}"
repo="${repo:-unknown-repo}"
scenario_id="${scenario_id:-unknown-scenario}"
caption="${caption:-manifest-fed smoke render}"

out_dir="renders/smoke"
out_file="$out_dir/${work}-smoke.mp4"
mkdir -p "$out_dir"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

card_text="$tmp_dir/card.txt"
cat > "$card_text" <<EOF
$title
source: $repo / $scenario_id
format: $format
style: $style
$caption
EOF

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "color=c=0x041E42:s=1280x720:d=6:r=24" \
  -f lavfi -i "anullsrc=channel_layout=stereo:sample_rate=48000" \
  -vf "drawtext=textfile=$card_text:fontcolor=white:fontsize=42:line_spacing=16:x=(w-text_w)/2:y=(h-text_h)/2,format=yuv420p" \
  -shortest \
  -c:v libx264 \
  -pix_fmt yuv420p \
  -c:a aac \
  -t 6 \
  "$out_file"

echo "$out_file"
