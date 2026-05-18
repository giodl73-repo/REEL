#!/usr/bin/env bash
set -euo pipefail

manifest="${1:-works/0001-ash-vale-last-road-before-winter/manifest.yaml}"

if [[ ! -f "$manifest" ]]; then
  echo "manifest not found: $manifest" >&2
  exit 2
fi

required_top_fields=(
  manifest_version
  work
  title
  source_scenario
  format
  style
  audience
  platforms
  continuity
  scenes
  shots
  audio
  captions
  renderer_assumptions
  exports
  review
)

missing=0
for field in "${required_top_fields[@]}"; do
  if ! grep -Eq "^${field}:" "$manifest"; then
    echo "missing required top-level field: $field" >&2
    missing=1
  fi
done

if [[ "$missing" -ne 0 ]]; then
  exit 3
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

extract_section() {
  local section="$1"
  awk -v section="$section" '
  function clean(line) {
    sub(/^[[:space:]]+(-[[:space:]]*)?[A-Za-z_]+:[[:space:]]*/, "", line)
    gsub(/\r/, "", line)
    gsub(/^"/, "", line)
    gsub(/"$/, "", line)
    return line
  }
  function value_or_dash(value) {
    return value == "" ? "-" : value
  }
  function emit() {
    if (id != "") {
      print value_or_dash(id) "\t" value_or_dash(scene_id) "\t" value_or_dash(start) "\t" value_or_dash(duration) "\t" value_or_dash(aspect) "\t" value_or_dash(filename)
    }
  }
  $0 == section ":" { in_section = 1; next }
  in_section && /^[A-Za-z_]+:/ {
    emit()
    id = scene_id = start = duration = aspect = filename = ""
    in_section = 0
    next
  }
  in_section && /^  - (id|name):/ {
    emit()
    id = clean($0)
    scene_id = start = duration = aspect = filename = ""
    next
  }
  in_section && id != "" && /^    scene_id:/ { scene_id = clean($0); next }
  in_section && id != "" && /^    start_seconds:/ { start = clean($0); next }
  in_section && id != "" && /^    duration_seconds:/ { duration = clean($0); next }
  in_section && id != "" && /^    target_duration_seconds:/ { duration = clean($0); next }
  in_section && id != "" && /^    aspect_ratio:/ { aspect = clean($0); next }
  in_section && id != "" && /^    filename:/ { filename = clean($0); next }
  END { emit() }
  ' "$manifest"
}

scenes_tsv="$tmp_dir/scenes.tsv"
shots_tsv="$tmp_dir/shots.tsv"
platforms_tsv="$tmp_dir/platforms.tsv"
exports_tsv="$tmp_dir/exports.tsv"

extract_section scenes > "$scenes_tsv"
extract_section shots > "$shots_tsv"
extract_section platforms > "$platforms_tsv"
extract_section exports > "$exports_tsv"

for parsed in "$scenes_tsv" "$shots_tsv" "$platforms_tsv" "$exports_tsv"; do
  if [[ ! -s "$parsed" ]]; then
    echo "manifest section parsed no entries: $(basename "$parsed" .tsv)" >&2
    exit 4
  fi
done

scene_total="$(awk -F '\t' '{ total += $4 } END { printf "%.3f", total }' "$scenes_tsv")"
shot_total="$(awk -F '\t' '{ total += $4 } END { printf "%.3f", total }' "$shots_tsv")"

if ! awk -v scenes="$scene_total" -v shots="$shot_total" 'BEGIN { exit (scenes == shots ? 0 : 1) }'; then
  echo "scene duration total ($scene_total) does not match shot duration total ($shot_total)" >&2
  exit 5
fi

if ! awk -F '\t' '
BEGIN { expected = 0; ok = 1 }
{
  if ($3 != expected) {
    printf "shot %s starts at %s, expected %s\n", $1, $3, expected > "/dev/stderr"
    ok = 0
  }
  expected += $4
}
END { exit (ok ? 0 : 1) }
' "$shots_tsv"; then
  exit 6
fi

while IFS=$'\t' read -r shot_id scene_id _rest; do
  if ! awk -F '\t' -v scene_id="$scene_id" '$1 == scene_id { found = 1 } END { exit (found ? 0 : 1) }' "$scenes_tsv"; then
    echo "shot $shot_id references unknown scene_id: $scene_id" >&2
    exit 7
  fi
done < "$shots_tsv"

while IFS=$'\t' read -r export_id _scene _start export_duration export_aspect export_filename; do
  platform_row="$(awk -F '\t' -v id="$export_id" '$1 == id { print; exit }' "$platforms_tsv")"
  if [[ -z "$platform_row" ]]; then
    echo "export $export_id has no matching platforms entry" >&2
    exit 8
  fi

  IFS=$'\t' read -r _platform_id _platform_scene _platform_start platform_duration platform_aspect _platform_filename <<< "$platform_row"
  if [[ "$export_aspect" != "$platform_aspect" ]]; then
    echo "export $export_id aspect ratio ($export_aspect) does not match platform ($platform_aspect)" >&2
    exit 9
  fi
  if [[ "$export_duration" != "$platform_duration" ]]; then
    echo "export $export_id duration ($export_duration) does not match platform target ($platform_duration)" >&2
    exit 10
  fi
  if [[ "$export_filename" == "-" ]]; then
    echo "export $export_id is missing filename" >&2
    exit 11
  fi
  if ! awk -v target="$export_duration" -v base="$shot_total" 'BEGIN { exit (target > 0 && target <= base ? 0 : 1) }'; then
    echo "export $export_id duration ($export_duration) must be positive and no longer than shot total ($shot_total)" >&2
    exit 12
  fi

  scale="$(awk -v target="$export_duration" -v base="$shot_total" 'BEGIN { printf "%.3f", target / base }')"
  printf 'export ok: %s %ss %s scale=%s filename=%s\n' "$export_id" "$export_duration" "$export_aspect" "$scale" "$export_filename"
done < "$exports_tsv"

printf 'manifest ok: %s\n' "$manifest"
printf 'timeline ok: scenes=%ss shots=%ss\n' "$scene_total" "$shot_total"
