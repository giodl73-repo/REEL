"""Verify a synthetic matched study and create captioned review derivatives.

Runs after the real scene engine has rendered baseline, revised and reduced.
Uses the compiler's revised frame schedule for baseline evidence as well.
These review derivatives do not replace delivery receipts or creative review.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from PIL import Image, ImageDraw, ImageFont


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def run(args: list[str], root: Path) -> None:
    subprocess.run(args, cwd=root, check=True, stdout=subprocess.DEVNULL)


def srt_time(seconds: int) -> str:
    return f"{seconds//3600:02}:{seconds//60%60:02}:{seconds%60:02},000"


def review(root: Path, font: Path, output_name: str) -> None:
    if Path(output_name).name != output_name or output_name in (".", ".."):
        raise ValueError("output name must be one local directory component")
    output = root / output_name
    output.mkdir(exist_ok=False)
    study, profile = read(root / "study.json"), read(root / "profile.json")
    variants = ["baseline", "revised", "reduced"]
    receipts = {v: read(root / f"render-{v}/receipt.json") for v in variants}
    evidence = read(root / "render-revised/motioncraft/evidence.json")
    # Reuse the compiler-derived schedule; no second authored timing schedule.
    indices = [f["frame_index"] for f in evidence["frames"]]
    stem_hashes = {}
    for stem in ["D.wav", "E.wav", "M.wav", "mix.wav"]:
        hashes = {v: sha(root / f"render-{v}" / stem) for v in variants}
        if len(set(hashes.values())) != 1:
            raise ValueError(f"matched study changed native {stem}: {hashes}")
        stem_hashes[stem] = hashes
    for v, receipt in receipts.items():
        plan = receipt["plan"]
        if plan["duration_samples"] != study["duration_samples"] or plan["frame_count"] != study["delivery_frames"]:
            raise ValueError(f"{v} has a different native clock")
        if plan["pictures"] != receipts["baseline"]["plan"]["pictures"]:
            raise ValueError(f"{v} changes picture selection timing")
        probe = subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0",
            "-count_frames", "-show_entries", "stream=width,height,nb_read_frames,r_frame_rate",
            "-of", "json", str(root / f"render-{v}/picture.mkv")])
        stream = json.loads(probe)["streams"][0]
        if (stream["width"], stream["height"], int(stream["nb_read_frames"]), stream["r_frame_rate"]) != (
                profile["width"], profile["height"], study["delivery_frames"], "30/1"):
            raise ValueError(f"{v} decoded video differs from required profile")
    captions, elapsed = [], 0
    for index, (seconds, title, subtitle, _) in enumerate(study["script"], 1):
        captions.append(f"{index}\n{srt_time(elapsed)} --> {srt_time(elapsed+seconds)}\n{title}\n{subtitle}\n")
        elapsed += seconds
    (output / "captions.srt").write_text("\n".join(captions), encoding="utf-8")
    caption_size = 10 if profile["height"] > profile["width"] else 12
    for v in variants:
        target = output / v
        target.mkdir()
        run(["ffmpeg", "-v", "error", "-nostdin", "-n", "-i", str(root / f"render-{v}/review.mp4"),
             "-an", "-vf", f"subtitles=captions.srt:force_style='FontName=Arial,FontSize={caption_size},MarginV=16,Outline=1'",
             "-c:v", "libx264", "-crf", "18", str(target / "muted-captioned.mp4")], output)
        selection = "+".join(f"eq(n\\,{i})" for i in indices)
        run(["ffmpeg", "-v", "error", "-nostdin", "-n", "-i", str(root / f"render-{v}/picture.mkv"),
             "-vf", f"select={selection}", "-fps_mode", "vfr", str(target / "sample-%03d.png")], root)
        for number, index in enumerate(indices, 1):
            (target / f"sample-{number:03d}.png").rename(target / f"frame-{index:08d}.png")
    # Labelled matched board at each native shot's reading midpoint.
    shots = receipts["baseline"]["plan"]["pictures"]
    thumb_width = 540 if profile["width"] > profile["height"] else 360
    thumb_height = round(thumb_width * profile["height"] / profile["width"])
    row_height = thumb_height + 36
    board = Image.new("RGB", (thumb_width * 3, row_height * len(shots)), "white")
    draw = ImageDraw.Draw(board)
    face = ImageFont.truetype(str(font), 18)
    chosen = []
    for row, shot in enumerate(shots):
        midpoint = (shot["start_frame"] + shot["end_frame"] - 1) // 2
        index = min((i for i in indices if shot["start_frame"] <= i < shot["end_frame"]), key=lambda i: abs(i-midpoint))
        chosen.append(index)
        for col, v in enumerate(variants):
            with Image.open(output / v / f"frame-{index:08d}.png") as frame:
                thumb = frame.convert("RGB").resize((thumb_width, thumb_height), Image.Resampling.LANCZOS)
            board.paste(thumb, (col*thumb_width, row*row_height+36))
            draw.text((col*thumb_width+8, row*row_height+8), f"{v} · frame {index} · shot {row+1}", font=face, fill="black")
    board.save(output / "matched-board.png")
    inventory = [{"path": str(p.relative_to(root)).replace("\\", "/"), "bytes": p.stat().st_size,
                  "sha256": sha(p)} for p in sorted(output.rglob("*")) if p.is_file()]
    report = {"schema": "reel.motioncraft-study-comparison.v1", "synthetic_only": True,
              "profile": profile, "native_clock_verified": True, "native_stems_identical": stem_hashes,
              "sampling_schedule_source": {"path": "render-revised/motioncraft/evidence.json", "sha256": sha(root / "render-revised/motioncraft/evidence.json")},
              "matched_board_indices": chosen, "frame_indices": indices,
              "captions": "Original study text; not a transcript of spoken dialogue",
              "scope": "Technical matching proof and review derivatives; no creative score or viewer approval",
              "artifacts": inventory}
    (output / "comparison.json").write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--output-name", default="review-comparison")
    args = parser.parse_args()
    review(args.root.resolve(), args.font.resolve(), args.output_name)
