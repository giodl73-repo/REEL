"""Generate an original, synthetic Motioncraft queue study for the real scene engine.

Requires Pillow and an explicitly supplied local font. Never uses CAIMITOS media.
Outputs are new-only. Compilation/rendering remain REEL CLI operations.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct

from PIL import Image, ImageDraw, ImageFont


def write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def asset(root: Path, logical: str, data: bytes) -> dict:
    sha = digest(data)
    target = root / "objects" / "sha256" / sha[:2] / sha
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        if target.read_bytes() != data:
            raise ValueError("existing object bytes differ")
    else:
        target.write_bytes(data)
    return {"logical_id": logical, "sha256": sha, "bytes": len(data),
            "cache_uri": f"cache://sha256/{sha}", "selection_state": "selected-private-production"}


def pcm(seconds: int, frequency: int = 0) -> bytes:
    count = seconds * 48000
    payload = bytearray()
    for sample in range(count):
        # An original brief cue; no recorded person or synthesized voice.
        value = int(400000 * math.sin(2 * math.pi * frequency * sample / 48000)
                    * (1 - sample / 2400)) if frequency and sample < 2400 else 0
        payload.extend(struct.pack("<i", value)[:3] * 2)
    return (b"RIFF" + struct.pack("<I", 36 + len(payload)) + b"WAVEfmt "
            + struct.pack("<IHHIIHH", 16, 1, 2, 48000, 288000, 6, 24)
            + b"data" + struct.pack("<I", len(payload)) + payload)


SCRIPT = [
    (3, "Why does a queue grow?", "Arrivals exceed processing", 1),
    (4, "More arrives than leaves", "Work accumulates in the queue", 4),
    (4, "Slow arrivals", "The worker can catch up", 2),
    (4, "Keep the queue manageable", "Control arrivals or add capacity", 0),
]


def artwork(width: int, height: int, index: int, font: Path) -> bytes:
    import io
    image = Image.new("RGB", (width, height), "#FAF7F2")
    draw = ImageDraw.Draw(image)
    portrait = height > width
    title_font = ImageFont.truetype(str(font), int(min(width, height) * 0.055))
    body_font = ImageFont.truetype(str(font), int(min(width, height) * 0.041))
    label_font = ImageFont.truetype(str(font), int(min(width, height) * 0.033))
    _, title, subtitle, queued = SCRIPT[index]
    def centered(text: str, y: float, face: ImageFont.FreeTypeFont) -> None:
        box = draw.textbbox((0, 0), text, font=face)
        draw.text(((width - (box[2] - box[0])) / 2, height * y), text, font=face, fill="#1F1E1B")
    centered(title, 0.12, title_font)
    centered(subtitle, 0.23 if portrait else 0.26, body_font)
    # Layout is authored separately for each profile, never a portrait crop.
    x0, y0, x1, y1 = width * 0.16, height * 0.43, width * 0.84, height * 0.64
    draw.rounded_rectangle((x0, y0, x1, y1), radius=12, outline="#D97757", width=4)
    size = min(width * 0.09, height * 0.085)
    for token in range(queued):
        left = x0 + width * 0.045 + token * width * 0.14
        draw.rounded_rectangle((left, y0 + (y1-y0-size)/2, left+size, y0+(y1-y0+size)/2),
                               radius=6, fill="#305C70")
    centered("QUEUE", 0.68, label_font)
    centered("ARRIVALS  →  QUEUE  →  WORKER", 0.79, label_font)
    stream = io.BytesIO()
    image.save(stream, format="PNG")
    return stream.getvalue()


def direction() -> dict:
    return {"purpose": "Land the focal composition, then hold for reading",
            "dominant_element": "picture", "working_fps": 30, "duration_frames": 120,
            "fit_native_duration": True,
            "protected_regions": [{"x": 0.1, "y": 0.1, "width": 0.8, "height": 0.8}],
            "visual_intent": {
                "palette_roles": {"base": "#FAF7F2", "ink": "#1F1E1B", "hero": "#D97757", "tokens": "#305C70"},
                "typography_roles": {"headline": {"font_family": "Explicit local font", "relative_height": 0.055, "weight": 400}},
                "reference_traits": ["Warm editorial hierarchy", "One focal queue", "Readable stillness"], "transition": "hard-cut"},
            "elements": [{"id": "picture", "role": "camera",
                "bounds": {"x": 0.1, "y": 0.1, "width": 0.8, "height": 0.8},
                "phases": [
                    {"id": "land", "kind": "entrance", "start_frame": 0, "end_frame": 14,
                     "curve": "ease-out", "zoom_from": 1, "zoom_to": 1.08},
                    {"id": "read", "kind": "hold", "start_frame": 15, "end_frame": 119,
                     "curve": "linear", "zoom_from": 1.08, "zoom_to": 1.08}]}]}


def generate(root: Path, font: Path, width: int, height: int) -> None:
    if root.exists():
        raise FileExistsError(f"refusing to replace {root}")
    root.mkdir(parents=True)
    bindings = {}
    cues, events, slots, selected_events, paths = [], [], [], [], {}
    for index, (seconds, title, subtitle, _) in enumerate(SCRIPT):
        cue, picture, sonic = f"cue-{index}", f"picture-{index}", f"sonic.{index}"
        voice = asset(root, cue, pcm(seconds))
        art = asset(root, picture, artwork(width, height, index, font))
        sound = asset(root, sonic, pcm(seconds, 450 + index*100))
        alignment = {"schema": "reel.scene-native-alignment.v1", "language": "en", "cue_id": cue,
                     "selected_take_sha256": voice["sha256"], "sample_rate": 48000,
                     "cue_end_sample": seconds * 48000, "semantic_markers": {"first": 0}}
        write(root / f"alignment-{index}.json", alignment)
        # Bind the exact serialized local alignment bytes, not a different JSON encoding.
        alignment_asset = asset(root, f"alignment-{index}", (root / f"alignment-{index}.json").read_bytes())
        bindings.update({cue: voice, picture: art, sonic: sound, f"alignment-{index}": alignment_asset})
        paths[cue] = f"alignment-{index}.json"
        cues.append({"cue_id": cue, "source_id": cue, "exact_text_sha256": digest((title+"\n"+subtitle).encode()),
                     "narration_slot_id": cue, "take_binding": cue, "phrase_alignment_binding": f"alignment-{index}"})
        events.append({"event_id": f"event-{index}", "cue_id": cue, "semantic_trigger_id": "first",
                       "picture_slot_id": picture, "picture_binding": picture,
                       "score": {"disposition": "silence"}, "sonic_bindings": [sonic], "vfx_bindings": []})
        for slot, lane, item in [(cue, "narration", voice), (picture, "picture", art)]:
            slots.append({"slot_id": slot, "beat_id": cue, "lane": lane, "disposition": "selected",
                          "selected_revision_id": "r1", "revisions": [{"revision_id": "r1", "asset":
                              {key: item[key] for key in ["logical_id", "cache_uri", "sha256"]}}]})
        selected_events.append({"event_id": f"event-{index}", "scene_id": "queue", "language": "en",
                                "narration": {"logical_id": cue, "sha256": voice["sha256"]},
                                "picture": {"logical_id": picture, "sha256": art["sha256"]},
                                "phrase_start_seconds": 0, "phrase_end_seconds": seconds})
    write(root / "scene.json", {"schema": "reel.scene-authoring.v1", "scene_id": "queue", "episode_id": "demo",
        "authoring_state": "ready-for-private-build", "source_scope_ids": [f"cue-{i}" for i in range(4)],
        "source_authority_id": "original-synthetic-study", "languages": {"en": {"cues": cues, "events": events}}})
    episode = {"schema": "reel.episode-authoring.v1", "episode_id": "demo", "season_id": "study",
               "authoring_state": "ready-for-private-build", "master_template_id": "master", "scene_policy_id": "study",
               "presentation": [], "score_palette": [], "scene_ids": ["queue"]}
    write(root / "episode-baseline.json", episode)
    episode["motion_direction"] = direction()
    write(root / "episode-revised.json", episode)
    episode["motion_direction"]["reduced_motion"] = True
    write(root / "episode-reduced.json", episode)
    write(root / "catalog.json", {"schema": "reel.scene-template-catalog.v1", "templates": []})
    write(root / "policy.json", {"schema": "reel.scene-policy.v1", "policy_id": "study",
        "target_composition_seconds_min": 1, "target_composition_seconds_max": 15,
        "hard_unchanged_composition_seconds_max": 15, "semantic_cuts_required": True})
    for scope, assets in [("season", {}), ("episode", {}), ("scene", bindings)]:
        write(root / f"{scope}-bindings.json", {"schema": "reel.scene-asset-bindings.v1",
              "scope_id": {"season": "study", "episode": "demo", "scene": "queue"}[scope], "assets": assets})
    graph = {"schema": "reel.semantic-assembly.v1", "lock": {"logical_id": "synthetic-selection", "sha256": digest(json.dumps(slots).encode())},
             "slots": slots, "events": selected_events,
             "nodes": [{"id": "queue", "inputs": [], "slots": [slot["slot_id"] for slot in slots],
                        "events": [event["event_id"] for event in selected_events]}], "presentation_targets": []}
    write(root / "graph.json", graph)
    write(root / "pointer.json", {"schema": "reel.selected-pointer.v1", "logical_id": "current", "selected_lock": graph["lock"]})
    write(root / "alignment-paths.json", paths)
    write(root / "profile.json", {"schema": "reel.scene-delivery-profile.v1", "width": width, "height": height,
        "sample_rate": 48000, "frame_rate_numerator": 30, "frame_rate_denominator": 1,
        "motion_safe_area": {"x": 0.05, "y": 0.05, "width": 0.9, "height": 0.9},
        "max_composition_samples": 720000, "score_gain_db": -18, "score_fade_in_samples": 0, "score_fade_out_samples": 0})
    for variant in ["baseline", "revised", "reduced"]:
        write(root / f"compile-{variant}.json", {"schema": "reel.scene-delivery-compile.v1", "catalog": "catalog.json",
            "episode": f"episode-{variant}.json", "scene": "scene.json", "policy": "policy.json",
            "season_bindings": "season-bindings.json", "episode_bindings": "episode-bindings.json", "scene_bindings": "scene-bindings.json",
            "graph": "graph.json", "pointer": "pointer.json", "alignment_paths": "alignment-paths.json", "profile": "profile.json",
            "language": "en", "delivery_id": f"queue-{variant}", "delivery_title": "Why a queue grows",
            "output_dir": f"compiled-{variant}"})
    write(root / "study.json", {"schema": "reel.motioncraft-study.v1", "synthetic_only": True,
        "script": SCRIPT, "duration_samples": 720000, "delivery_frames": 450,
        "font_sha256": digest(font.read_bytes()), "font_scope": "explicit local font; no font redistribution",
        "dialogue": "intentional silence; explanatory text is on-screen", "effects": "original mathematical short cues",
        "comparison": "same claims, selected pictures, source audio, duration and output profile; revised camera phases only",
        "publication": "not-authorized"})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    args = parser.parse_args()
    if not args.font.is_file() or args.width < 640 or args.height < 640 or args.width > 1920 or args.height > 1920:
        parser.error("supply an existing local font and a supported study canvas")
    generate(args.output, args.font, args.width, args.height)
