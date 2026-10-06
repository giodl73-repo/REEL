"""Independent native-PCM/clock check for CAIMITOS's existing sanitized canary.

This checks a baseline made by the actual consumer adapter. It does not add
motion to that adapter, grant selection authority or claim upgrade readiness.
"""
from __future__ import annotations
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import wave


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check(root: Path, source: Path, adapter: Path) -> dict:
    original = read(source)
    if original.get("schema") != "caimitos.reel-adapter-canary.v1" or not original.get("synthetic_only"):
        raise ValueError("only the declared synthetic consumer canary is supported")
    imported = read(root / "adapter-receipt.json")
    if imported["source"]["sha256"] != sha(source):
        raise ValueError("adapter receipt is not bound to this source")
    contract = read(root / "contract.json")
    job = read(root / "job.json")
    rendered = read(root / "render/receipt.json")
    if read(root / "check-command-report.json") != rendered:
        raise ValueError("successful independent scene-delivery check report missing or differs")
    plan = rendered["plan"]
    if [c["duration_samples"] for c in contract["cues"]] != [48001, 47999]:
        raise ValueError("native cue clocks changed")
    if (plan["duration_samples"], plan["sample_rate"], plan["frame_count"], plan["fps_numerator"], plan["fps_denominator"]) != (96000, 48000, 48, 24, 1):
        raise ValueError("native delivery/frame clock changed")
    if [p["attachment_id"] for p in job["pictures"]] != ["picture-1", "picture-2"] or any("motion" in p for p in job["pictures"]):
        raise ValueError("legacy media identity/motion behavior changed")
    if [(p["start_sample"], p["end_sample"], p["start_frame"], p["end_frame"]) for p in plan["pictures"]] != [(0,48001,0,24),(48001,96000,24,48)]:
        raise ValueError("picture partition changed")
    effect = original["effect"]
    rows = [a for a in plan["audio"] if a["attachment_id"] == effect["id"]]
    if len(rows) != 1 or (rows[0]["start_sample"], rows[0]["end_sample"]) != (123,139):
        raise ValueError("native effect anchor changed")
    pcm = {}
    for name in ("D.wav", "E.wav", "M.wav", "mix.wav"):
        with wave.open(str(root / "render" / name), "rb") as stream:
            if (stream.getnchannels(), stream.getsampwidth(), stream.getframerate(), stream.getnframes()) != (2,3,48000,96000):
                raise ValueError(f"decoded native stem clock changed: {name}")
            pcm[name] = stream.readframes(96000)
    silent = b"\0" * (96000 * 6)
    pulse = struct.pack("<i", effect["amplitude"])[:3] * 2
    expected = bytearray(silent)
    expected[123*6:139*6] = pulse * 16
    if pcm["D.wav"] != silent or pcm["M.wav"] != silent or pcm["E.wav"] != expected or pcm["mix.wav"] != expected:
        raise ValueError("independent decoded PCM placement differs from the consumer source")
    probe = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-count_frames", "-select_streams", "v:0",
        "-show_entries", "stream=nb_read_frames,r_frame_rate", "-of", "json", str(root / "render/picture.mkv")]))["streams"][0]
    if (int(probe["nb_read_frames"]), probe["r_frame_rate"]) != (48, "24/1"):
        raise ValueError("decoded picture clock differs")
    residual = Fraction(48001*24,48000) - 24
    return {"schema": "reel.motioncraft-consumer-baseline-check.v1", "passed": True,
        "adapter_script_sha256": sha(adapter), "source_sha256": sha(source),
        "job_sha256": sha(root / "job.json"), "scene_receipt_sha256": sha(root / "render/receipt.json"),
        "native_cue_samples": [48001,47999], "duration_samples": 96000, "decoded_picture_frames": 48,
        "effect_start_sample": 123, "effect_end_sample_exclusive": 139,
        "first_boundary_frame_residual": {"numerator": residual.numerator, "denominator": residual.denominator},
        "picture_attachment_ids": [p["attachment_id"] for p in job["pictures"]],
        "native_pcm_sha256": {name: hashlib.sha256(data).hexdigest() for name,data in pcm.items()},
        "scope": "Existing actual CAIMITOS adapter baseline; motion adoption/new-upgrade-rollback remain unproven"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--adapter-script", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = check(args.root.resolve(), args.source.resolve(), args.adapter_script.resolve())
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
