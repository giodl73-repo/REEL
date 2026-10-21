"""Offline exact-text alignment worker for REEL's existing admitted jobs.

Produces native word-evidence candidates, never selects a take or approves its
performance. Python/WhisperX and all declared models must already be supplied.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import math
from numbers import Real
import os
from pathlib import Path
import re
import shutil
import struct
import sys

from local_tts_worker import digest, local_path, offline, verify

SCHEMA = "reel.local-word-alignment.v1"
MODELS = {
    "en": ("WAV2VEC2_ASR_BASE_960H", "wav2vec2_fairseq_base_ls960_asr_ls960.pth"),
    "es": ("VOXPOPULI_ASR_BASE_10K_ES", "wav2vec2_voxpopuli_base_10k_asr_es.pt"),
}


def tokens(text):
    return re.findall(r"[^\W_]+", text.lower(), re.UNICODE)


def wave_clock(path):
    """Read integer PCM clocks, including extensible PCM on Python 3.11."""
    size = path.stat().st_size
    with path.open("rb") as stream:
        header = stream.read(12)
        if len(header) != 12 or header[:4] != b"RIFF" or header[8:] != b"WAVE":
            raise ValueError("Integer PCM RIFF WAV required")
        limit = struct.unpack_from("<I", header, 4)[0] + 8
        if limit != size:
            raise ValueError("WAV RIFF length differs")
        fmt, data_bytes = None, None
        while stream.tell() < limit:
            chunk = stream.read(8)
            if len(chunk) != 8:
                raise ValueError("Truncated WAV chunk")
            kind, length = struct.unpack("<4sI", chunk)
            end = stream.tell() + length
            if end + (length & 1) > limit:
                raise ValueError("WAV chunk exceeds RIFF length")
            if kind == b"fmt ":
                if fmt is not None or not 16 <= length <= 4096:
                    raise ValueError("Invalid WAV format chunk")
                raw = stream.read(length)
                tag, channels, rate, byte_rate, block, bits = struct.unpack_from("<HHIIHH", raw)
                if tag == 65534:
                    if (length < 40 or struct.unpack_from("<H", raw, 16)[0] < 22
                            or raw[24:40] != bytes.fromhex("0100000000001000800000aa00389b71")
                            or not 0 < struct.unpack_from("<H", raw, 18)[0] <= bits):
                        raise ValueError("Unsupported extensible WAV subtype")
                elif tag != 1:
                    raise ValueError("Integer PCM WAV required")
                if (channels != 1 or bits not in {16, 24, 32} or rate <= 0
                        or block != channels * (bits // 8) or byte_rate != rate * block):
                    raise ValueError("Native WAV format differs")
                fmt = rate, block
            elif kind == b"data":
                if data_bytes is not None:
                    raise ValueError("Multiple WAV data chunks")
                data_bytes = length
            stream.seek(end + (length & 1))
        if fmt is None or data_bytes is None or data_bytes % fmt[1]:
            raise ValueError("Missing or incomplete PCM frames")
        return fmt[0], data_bytes // fmt[1]


def validate(root, request):
    fields = {"schema", "device", "models", "items", "output", "runtime"}
    if set(request) != fields or request["schema"] != SCHEMA:
        raise ValueError("Unsupported local word-alignment request")
    if request["device"] not in {"cpu", "cuda"}:
        raise ValueError("Explicit CPU/CUDA device required")
    items = request["items"]
    if not isinstance(items, list) or not 1 <= len(items) <= 512:
        raise ValueError("Bounded nonempty alignment batch required")
    output = local_path(root, request["output"])
    if output.exists():
        raise ValueError("Output already exists")
    languages = {item.get("language") for item in items}
    if not languages.issubset(MODELS) or set(request["models"]) != languages:
        raise ValueError("Exact supplied language model set required")
    models = {}
    for language, descriptor in request["models"].items():
        model = verify(root, descriptor)
        if model.name != MODELS[language][1]:
            raise ValueError("Model filename differs from pinned backend bundle")
        models[language] = model
    allowed = {v.resolve() for v in models.values()}
    for parent in {v.parent for v in models.values()}:
        if any(f.resolve() not in allowed for f in parent.iterdir()):
            raise ValueError("Unpinned model-directory entry")
    ids = set()
    for item in items:
        expected = {"id", "language", "cue_id", "text", "text_sha256", "audio",
                    "sample_rate", "cue_end_sample"}
        if set(item) != expected or not isinstance(item["id"], str) or not item["id"] or item["id"] in ids:
            raise ValueError("Duplicate or invalid item identity")
        ids.add(item["id"])
        if not isinstance(item["cue_id"], str) or not item["cue_id"]:
            raise ValueError("Missing native cue identity")
        text = item["text"]
        if not isinstance(text, str) or not tokens(text) or len(text.encode()) > 8192:
            raise ValueError("Invalid bounded source text")
        if hashlib.sha256(text.encode()).hexdigest() != item["text_sha256"]:
            raise ValueError("Source text digest differs")
        audio = verify(root, item["audio"])
        if audio == output or audio.resolve() in allowed:
            raise ValueError("Input/output alias")
        rate, frames = wave_clock(audio)
        if (rate != item["sample_rate"] or frames != item["cue_end_sample"]
                or not 0 < frames <= rate * 120):
            raise ValueError("Native WAV clock/format differs")
    runtime = request["runtime"]
    if set(runtime) != {"python_sha256", "module_versions", "alignment_module_sha256", "ffmpeg_sha256"}:
        raise ValueError("Pinned runtime inventory required")
    return output, models


def evidence(item, aligned):
    words = []
    previous = -1
    for entry in aligned:
        word = entry.get("word", "")
        if not tokens(word):
            continue
        start, end = entry.get("start"), entry.get("end")
        if (not isinstance(start, Real) or isinstance(start, bool)
                or not isinstance(end, Real) or isinstance(end, bool)
                or not math.isfinite(start) or not math.isfinite(end) or start < 0 or end <= start):
            raise ValueError("Unmeasured or invalid word clock")
        first, last = round(start * item["sample_rate"]), round(end * item["sample_rate"])
        if first < previous or last <= first or last > item["cue_end_sample"]:
            raise ValueError("Word clock outside selected take or backward")
        words.append({"word": word, "start_sample": first, "end_sample": last})
        previous = first
    if [t for word in words for t in tokens(word["word"])] != tokens(item["text"]):
        raise ValueError("Complete word evidence differs from exact source text")
    return {"schema": "reel.scene-word-timing-evidence.v1", "language": item["language"],
            "cue_id": item["cue_id"], "selected_take_sha256": item["audio"]["sha256"],
            "sample_rate": item["sample_rate"], "cue_end_sample": item["cue_end_sample"], "words": words}


class WhisperXBackend:
    def __init__(self, request, models):
        import whisperx.alignment as alignment
        from whisperx.audio import load_audio
        runtime = request["runtime"]
        if digest(Path(sys.executable).resolve()) != runtime["python_sha256"]:
            raise ValueError("Python runtime differs")
        if digest(Path(alignment.__file__)) != runtime["alignment_module_sha256"]:
            raise ValueError("Alignment implementation differs")
        for module, version in runtime["module_versions"].items():
            if importlib.metadata.version(module) != version:
                raise ValueError("Runtime module version differs: " + module)
        ffmpeg = shutil.which("ffmpeg")
        if ffmpeg is None or digest(Path(ffmpeg)) != runtime["ffmpeg_sha256"]:
            raise ValueError("FFmpeg runtime differs")
        self.alignment, self.load_audio = alignment, load_audio
        self.request, self.models, self.loaded = request, models, {}

    def __call__(self, item, audio):
        language, device = item["language"], self.request["device"]
        if language not in self.loaded:
            self.loaded[language] = self.alignment.load_align_model(
                language, device, model_name=MODELS[language][0],
                model_dir=str(self.models[language].parent), model_cache_only=True)
        model, metadata = self.loaded[language]
        result = self.alignment.align(
            [{"start": 0.0, "end": item["cue_end_sample"] / item["sample_rate"], "text": item["text"]}],
            model, metadata, self.load_audio(str(audio)), device,
            interpolate_method="nearest", return_char_alignments=False, print_progress=False)
        return result["word_segments"]


def align(root, request, backend_factory=WhisperXBackend):
    output, models = validate(root, request)
    os.environ.update({"HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1",
                       "HF_DATASETS_OFFLINE": "1", "XDG_CACHE_HOME": str(root / "scratch")})
    offline()
    backend = backend_factory(request, models)
    results = []
    for item in request["items"]:
        raw = backend(item, local_path(root, item["audio"]["path"]))
        entry = {"id": item["id"], "audio_sha256": item["audio"]["sha256"],
                 "text_sha256": item["text_sha256"], "raw_aligned_words": raw}
        try:
            entry["evidence"] = evidence(item, raw)
            entry["state"] = "candidate-word-evidence"
        except ValueError as error:
            entry["state"], entry["hold"] = "word-evidence-held", str(error)
        results.append(entry)
    report = {"schema": "reel.local-word-alignment-receipt.v1", "results": results,
              "candidate_count": sum(r["state"] == "candidate-word-evidence" for r in results),
              "held_count": sum(r["state"] == "word-evidence-held" for r in results),
              "human_listening_approval": False, "performance_selected": False,
              "source_audio_modified": False, "remote_transfer": False,
              "models": request["models"], "runtime": request["runtime"]}
    encoded = (json.dumps(report, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    if len(encoded) > 16 * 1024 * 1024:
        raise ValueError("Alignment output exceeds declared bound")
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as stream:
        stream.write(encoded)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("request")
    args = parser.parse_args()
    request_path = Path(args.request).resolve()
    report = align(request_path.parent, json.loads(request_path.read_bytes()))
    print(json.dumps({"candidate_count": report["candidate_count"], "held_count": report["held_count"]}))
