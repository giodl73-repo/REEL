"""Offline Chatterbox worker for REEL's existing admitted, hash-bound jobs.

The caller owns authorization and supplies private inputs. This worker never
resolves caches, downloads models, selects a performance, or creates a clone.
"""
from __future__ import annotations

import argparse
import array
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import random
import socket
import sys
import wave

SCHEMA = "reel.local-tts.v1"
REQUIRED_MODEL = {
    "ve.pt", "s3gen.pt", "t3_mtl23ls_v2.safetensors",
    "grapheme_mtl_merged_expanded_v1.json",
}
OPTIONAL_MODEL = {"conds.pt", "Cangjie5_TC.json", "CACHEDIR.TAG"}
LANGUAGES = set("ar da de el en es fi fr he hi it ja ko ms nl no pl pt ru sv sw tr zh".split())
SETTINGS = {"exaggeration", "cfg_weight", "temperature", "repetition_penalty", "min_p", "top_p"}


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def local_path(root, name):
    if not isinstance(name, str) or not name or len(name) > 200 or "\\" in name or ":" in name:
        raise ValueError("Expected a bounded workspace-relative path")
    parts = name.split("/")
    if PurePosixPath(name).is_absolute() or any(p in ("", ".", "..") or p.strip() != p for p in parts):
        raise ValueError("Unsafe workspace-relative path")
    target = root
    for part in parts:
        target = target / part
        if target.is_symlink():
            raise ValueError("Symlink input/output is not allowed")
    if not target.resolve().is_relative_to(root.resolve()):
        raise ValueError("Path escapes workspace")
    return target


def verify(root, asset):
    if set(asset) != {"path", "sha256", "bytes"}:
        raise ValueError("Input requires exact path/hash/bytes")
    h = asset["sha256"]
    if not isinstance(h, str) or len(h) != 64 or any(c not in "0123456789abcdef" for c in h):
        raise ValueError("Invalid input digest")
    size = asset["bytes"]
    if type(size) is not int or not 0 < size <= 8 * 1024**3:
        raise ValueError("Invalid input byte bound")
    path = local_path(root, asset["path"])
    if not path.is_file() or path.stat().st_size != size or digest(path) != h:
        raise ValueError("Input byte/hash mismatch")
    return path


def validate(root, request):
    expected = {"schema", "text", "text_sha256", "language", "seed", "device", "sample_rate",
                "max_seconds", "settings", "authorization", "reference", "model_files", "output", "receipt"}
    if set(request) != expected or request["schema"] != SCHEMA:
        raise ValueError("Unsupported local TTS request")
    text = request["text"]
    if not isinstance(text, str) or not text.strip() or len(text.encode("utf-8")) > 8192:
        raise ValueError("Invalid bounded text")
    if hashlib.sha256(text.encode("utf-8")).hexdigest() != request["text_sha256"]:
        raise ValueError("Text digest mismatch")
    if request["language"] not in LANGUAGES or request["device"] not in {"cpu", "cuda", "mps"}:
        raise ValueError("Unsupported language/device")
    if type(request["seed"]) is not int or not 0 <= request["seed"] < 2**32:
        raise ValueError("Invalid explicit seed")
    if request["sample_rate"] not in {24000, 44100, 48000}:
        raise ValueError("Unsupported sample rate")
    if type(request["max_seconds"]) is not int or not 1 <= request["max_seconds"] <= 120:
        raise ValueError("Invalid output duration bound")
    settings = request["settings"]
    if set(settings) != SETTINGS or any(type(v) not in (float, int) or not math.isfinite(v) for v in settings.values()):
        raise ValueError("Explicit finite generation settings required")
    if any(not 0 <= settings[k] <= 2 for k in ["exaggeration", "cfg_weight"]):
        raise ValueError("Invalid conditioning settings")
    if not 0 < settings["temperature"] <= 2 or not 0 < settings["repetition_penalty"] <= 10:
        raise ValueError("Invalid sampling settings")
    if not 0 <= settings["min_p"] <= 1 or not 0 < settings["top_p"] <= 1:
        raise ValueError("Invalid probability settings")
    auth = request["authorization"]
    if set(auth) != {"local_synthesis", "remote_transfer", "public_release", "decision"}:
        raise ValueError("Explicit local authorization scope required")
    if auth["local_synthesis"] is not True or auth["remote_transfer"] is not False or auth["public_release"] is not False:
        raise ValueError("This worker requires an expressly private local-only scope")
    decision = verify(root, auth["decision"])
    reference = verify(root, request["reference"])
    models = request["model_files"]
    if not REQUIRED_MODEL <= set(models) or set(models) - REQUIRED_MODEL - OPTIONAL_MODEL:
        raise ValueError("Unsupported/incomplete local model file set")
    model_paths = {name: verify(root, asset) for name, asset in models.items()}
    parents = {path.parent for path in model_paths.values()}
    if len(parents) != 1 or any(path.name != name for name, path in model_paths.items()):
        raise ValueError("Model inputs must retain exact flat filenames in one directory")
    model_dir = next(iter(parents))
    if {f.name for f in model_dir.iterdir()} != set(models):
        raise ValueError("Unpinned model directory entry")
    output, receipt = [local_path(root, request[key]) for key in ["output", "receipt"]]
    inputs = {decision.resolve(), reference.resolve(), *(f.resolve() for f in model_paths.values())}
    if output == receipt or any(f.resolve() in inputs or f.exists() for f in [output, receipt]):
        raise ValueError("Output aliases an input or already exists")
    return reference, model_dir, output, receipt


def offline():
    for name in ["HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_DATASETS_OFFLINE"]:
        os.environ[name] = "1"
    def denied(*args, **kwargs):
        raise RuntimeError("Network connections are disabled for local TTS")
    socket.socket.connect = denied
    socket.socket.connect_ex = denied
    socket.create_connection = denied


def synthesize(request, reference, model_dir):
    # Imported only after source bytes and explicit scope have passed validation.
    import numpy as np
    import torch
    import torchaudio
    from chatterbox.mtl_tts import ChatterboxMultilingualTTS
    random.seed(request["seed"])
    np.random.seed(request["seed"])
    torch.manual_seed(request["seed"])
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(request["seed"])
    model = ChatterboxMultilingualTTS.from_local(model_dir, request["device"])
    audio = model.generate(text=request["text"], language_id=request["language"],
                           audio_prompt_path=str(reference), **request["settings"])
    if audio.ndim != 2 or audio.shape[0] != 1:
        raise ValueError("Expected mono model output")
    if model.sr != request["sample_rate"]:
        audio = torchaudio.functional.resample(audio, model.sr, request["sample_rate"])
    return audio.detach().cpu().flatten().tolist()


def render(root, request, backend=synthesize):
    reference, model_dir, output, receipt = validate(root, request)
    offline()
    samples = backend(request, reference, model_dir)
    if not samples or len(samples) > request["max_seconds"] * request["sample_rate"]:
        raise ValueError("Empty or over-duration model output")
    if any(not math.isfinite(x) for x in samples):
        raise ValueError("Nonfinite model output")
    peak = max(abs(x) for x in samples)
    if peak == 0:
        raise ValueError("Silent model output")
    gain = min(1.0, 0.98 / peak)
    pcm = array.array("h", (round(x * gain * 32767) for x in samples))
    if sys.byteorder != "little":
        pcm.byteswap()
    output.parent.mkdir(parents=True, exist_ok=True)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as stream:
        with wave.open(stream, "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(request["sample_rate"])
            wav.writeframes(pcm.tobytes())
    record = {"schema": "reel.local-tts-receipt.v1", "request_text_sha256": request["text_sha256"],
              "reference_sha256": request["reference"]["sha256"],
              "model_sha256": {k: v["sha256"] for k, v in request["model_files"].items()},
              "authorization_sha256": request["authorization"]["decision"]["sha256"],
              "worker_sha256": digest(Path(__file__)), "language": request["language"],
              "seed": request["seed"], "settings": request["settings"], "device": request["device"],
              "output_sha256": digest(output), "output_bytes": output.stat().st_size,
              "frames": len(samples), "sample_rate": request["sample_rate"], "channels": 1,
              "pcm_bits": 16, "source_peak": peak, "gain": gain,
              "network_enabled": False, "human_listening_approval": False, "selection_changed": False}
    with receipt.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(record, stream, ensure_ascii=False, separators=(",", ":"))
        stream.write("\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("request")
    args = parser.parse_args()
    path = local_path(Path.cwd(), args.request)
    if path.stat().st_size > 64 * 1024:
        raise ValueError("Request exceeds metadata bound")
    request = json.loads(path.read_text(encoding="utf-8"))
    render(Path.cwd(), request)
    print("Local-only TTS candidate complete; no performance selection or approval")


if __name__ == "__main__":
    main()
