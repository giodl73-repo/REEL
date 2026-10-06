"""Compare a verified caption-reserved study derivative to its parent study.

Run REEL scene-delivery-check first. This independent comparison verifies the
only job change, exact native stems and indexed clear-band pixels. It does not
replace REEL receipt validation, create captions or award creative approval.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def check(parent: Path, reserved: Path, output: Path) -> None:
    if output.exists():
        raise FileExistsError(output)
    job_path = reserved / "compiled-revised/job.json"
    job = read(job_path)
    layout = job.pop("caption_picture_layout", None)
    if not layout or layout["layout"] != "reserve-caption-band":
        raise ValueError("explicit caption reservation required")
    if job != read(parent / "compiled-revised/job.json"):
        raise ValueError("reservation changed more than the explicit layout")
    width, height = job["width"], job["height"]
    band_y = 900 * height // 1280 if height > width else 520 * height // 720
    evidence_path = reserved / "render-revised/motioncraft/evidence.json"
    evidence = read(evidence_path)
    expected_region = {"x": 0, "y": 0, "width": width, "height": band_y}
    if evidence.get("picture_layout") != {
        "strategy": "reserve-caption-band", "picture_region": expected_region
    } or evidence["job_sha256"] != sha(job_path):
        raise ValueError("review geometry or exact job binding differs")
    stems = {}
    for stem in ("D.wav", "E.wav", "M.wav", "mix.wav"):
        path = reserved / "render-revised" / stem
        if path.read_bytes() != (parent / "render-revised" / stem).read_bytes():
            raise ValueError(f"native audio changed: {stem}")
        stems[stem] = sha(path)
    samples = []
    if not 1 <= len(evidence["frames"]) <= 256 or band_y + 4 >= height:
        raise ValueError("invalid review sample budget or picture band")
    for entry in evidence["frames"]:
        name = f"frame-{entry['frame_index']:08d}.png"
        if entry["path"] != name:
            raise ValueError("indexed frame path differs")
        with Image.open(reserved / "render-revised/motioncraft" / name) as source:
            image = source.convert("RGB")
            if image.size != (width, height):
                raise ValueError("review frame dimensions differ")
            # Exclude four boundary rows where YUV conversion can blend chroma.
            if max(image.crop((0, band_y + 4, width, height)).tobytes()) > 2:
                raise ValueError(f"picture enters reserved band at frame {entry['frame_index']}")
            if max(image.crop((0, 0, width, band_y)).tobytes()) <= 20:
                raise ValueError("study picture missing from reserved viewport")
        samples.append(entry["frame_index"])
    report = {
        "schema": "reel.motioncraft-caption-check.v1",
        "caption_picture_layout": layout, "picture_region": expected_region,
        "only_job_change": "caption_picture_layout", "job_sha256": sha(job_path),
        "native_stems_identical": stems,
        "verified_clear_band_sample_indices": samples,
        "evidence_sha256": sha(evidence_path),
        "scope": "Exact selected-job/stem comparison and indexed pixel checks; sampled clear picture band, not caption text or creative approval",
        "passed": True,
    }
    with output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("parent", type=Path)
    parser.add_argument("reserved", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    check(args.parent.resolve(), args.reserved.resolve(), args.output.resolve())
