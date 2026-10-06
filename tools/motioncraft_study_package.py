"""Package/hydrate synthetic Motioncraft studies using REEL's inventory contract.

This is a study transport helper, not CAIMITOS source selection or its adapter.
All selected inputs are exact content-addressed bytes. Outputs are new-only.
Run REEL production-package-receipt/check separately for authoritative receipts.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess


def sha(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024*1024), b""):
            value.update(chunk)
    return value.hexdigest()


def read(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def write(path: Path, value: object) -> None:
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)


def inside(root: Path, relative: str) -> Path:
    parts = relative.split("/")
    if not relative or any(p in ("", ".", "..") or "\\" in p or ":" in p for p in parts):
        raise ValueError(f"invalid portable path: {relative}")
    path = root.joinpath(*parts).resolve(strict=True)
    if not path.is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError(f"path escapes package or is not a file: {relative}")
    return path


def kind(path: Path) -> str:
    if "objects" in path.parts:
        return "source-asset"
    if path.suffix == ".wav":
        return "render-audio"
    if path.suffix in (".mp4", ".mkv"):
        return "render-video"
    if path.suffix == ".srt":
        return "captions"
    if path.suffix == ".png" or "motioncraft" in path.parts or "review-comparison-v2" in path.parts or "cadence-r1" in path.parts:
        return "review-evidence"
    if path.name.startswith("caption-check") or path.name in ("reproduction-check.json", "caption-derivation.json"):
        return "review-evidence"
    if path.name.startswith("episode-") or path.name == "scene.json":
        return "craft-plan"
    if path.name == "job.json":
        return "choreography"
    if "receipt" in path.name:
        return "department-receipt"
    return "production-manifest"


def validate_cadence(root: Path, variant: str) -> None:
    report = read(root / f"cadence-r1/{variant}.json")
    if (report.get("schema") != "reel.motioncraft-cadence.v1"
        or report.get("passed") is not True
        or not report.get("shots")
        or any(shot.get("passed") is not True for shot in report["shots"])
        or report.get("job_sha256") != sha(root / f"compiled-{variant}/job.json")
        or report.get("picture_sha256") != sha(root / f"render-{variant}/picture.mkv")):
        raise ValueError("cadence report failed or differs from inventoried render/job")


def pack(studies: dict[str, Path], output: Path,
         caption_studies: dict[str, Path] | None = None,
         producer: Path | None = None) -> None:
    if output.exists():
        raise FileExistsError(output)
    if set(studies) != {"landscape", "portrait"}:
        raise ValueError("both portable study profiles are required")
    caption_studies = caption_studies or {}
    if caption_studies and set(caption_studies) != set(studies):
        raise ValueError("both caption-reserved profiles are required")
    if caption_studies and producer is None:
        raise ValueError("integrated caption package requires a producer pin")
    combined = dict(studies)
    combined.update({f"caption-{profile}": root for profile, root in caption_studies.items()})
    selected, transfers = {}, []
    for profile, root in combined.items():
        study = read(root / "study.json")
        caption = profile.startswith("caption-")
        if not study.get("synthetic_only"):
            raise ValueError("only verified synthetic studies may use this helper")
        if caption:
            parent = studies[profile.removeprefix("caption-")]
            job_path = root / "compiled-revised/job.json"
            job = read(job_path)
            layout = job.pop("caption_picture_layout", None)
            check = read(root / "caption-check.json")
            if (not layout or layout.get("layout") != "reserve-caption-band"
                or job != read(parent / "compiled-revised/job.json")
                or check.get("passed") is not True
                or check.get("job_sha256") != sha(job_path)
                or check.get("evidence_sha256") != sha(root / "render-revised/motioncraft/evidence.json")):
                raise ValueError("caption derivative check or parent binding differs")
            for stem in ("D.wav", "E.wav", "M.wav", "mix.wav"):
                if (sha(root / "render-revised" / stem) != sha(parent / "render-revised" / stem)
                    or check.get("native_stems_identical", {}).get(stem) != sha(root / "render-revised" / stem)):
                    raise ValueError("caption derivative changed native audio")
            variants = ("revised",)
            directories = ["compiled-revised", "render-revised", "cadence-r1"]
        else:
            comparison = read(root / "review-comparison-v2/comparison.json")
            if not comparison.get("native_clock_verified"):
                raise ValueError("study comparison has not verified the native clock")
            variants = ("baseline", "revised", "reduced")
            directories = [f"{prefix}-{v}" for prefix in ("compiled", "render") for v in variants] + ["review-comparison-v2", "cadence-r1"]
        for variant in variants:
            validate_cadence(root, variant)
        assets = []
        seen = set()
        for scope in ("season", "episode", "scene"):
            for binding_id, item in read(root / f"{scope}-bindings.json")["assets"].items():
                digest = item["sha256"]
                if not re.fullmatch("[0-9a-f]{64}", digest) or item["cache_uri"] != f"cache://sha256/{digest}":
                    raise ValueError("malformed content-addressed selected asset")
                relative = f"objects/sha256/{digest[:2]}/{digest}"
                source = inside(root, relative)
                if sha(source) != digest or source.stat().st_size != item["bytes"]:
                    raise ValueError("selected asset hash/byte count mismatch")
                assets.append({"binding_id": binding_id, "scope": scope,
                    "logical_id": item["logical_id"], "sha256": digest, "bytes": item["bytes"],
                    "cache_uri": item["cache_uri"], "path": f"studies/{profile}/{relative}",
                    "selection_state": item["selection_state"],
                    "provenance": "Original synthetic local graphic/audio or native alignment; no family media"})
                if relative not in seen:
                    transfers.append((source, f"studies/{profile}/{relative}"))
                    seen.add(relative)
        authored = []
        for source in sorted(root.glob("*.json")):
            relative = f"studies/{profile}/{source.name}"
            transfers.append((inside(root, source.name), relative))
            # Prior transport/check receipts are evidence, not clean inputs.
            # Replaying hydration-receipt.json would collide with the fresh
            # package-bound receipt after copying files into the destination.
            if source.name not in {"hydration-receipt.json", "reproduction-check.json", "caption-derivation.json"} and not source.name.startswith("caption-check"):
                authored.append(relative)
        for dirname in directories:
            directory = root / dirname
            if not directory.is_dir():
                raise ValueError(f"required study output missing: {directory}")
            for source in sorted(directory.rglob("*")):
                if source.is_file():
                    rel = source.relative_to(root).as_posix()
                    transfers.append((inside(root, rel), f"studies/{profile}/{rel}"))
        selected[profile] = {"assets": assets, "authoring_files": authored,
            "expected_compiled_directory": f"studies/{profile}/compiled-revised",
            "native_duration_samples": study["duration_samples"], "delivery_frames": study["delivery_frames"]}
    if producer is not None:
        pin = read(producer)
        if (pin.get("schema") != "reel.motioncraft-producer.v1"
            or not re.fullmatch("[0-9a-f]{40}", pin.get("source_commit", ""))
            or not pin.get("binaries")
            or any(not re.fullmatch("[0-9a-f]{64}", item.get("sha256", "")) for item in pin["binaries"])):
            raise ValueError("invalid producer identity pin")
        transfers.append((producer.resolve(strict=True), "producer.json"))
    destinations = [relative for _, relative in transfers]
    if len(destinations) != len(set(destinations)):
        raise ValueError("duplicate package path")
    # Preflight all paths/selected inputs before creating the new package.
    output.mkdir(parents=True)
    components = []
    for index, (source, relative) in enumerate(transfers):
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            raise ValueError(f"duplicate package path: {relative}")
        shutil.copyfile(source, target)
        components.append({"id": f"file-{index:04}", "kind": kind(Path(relative)),
                           "path": relative, "sha256": sha(target), "required": True})
        if relative == "producer.json":
            components[-1]["kind"] = "department-packet"
    hydration = {"schema": "reel.motioncraft-study-hydration.v1", "synthetic_only": True,
        "profiles": selected, "scope": "Transport exact inputs; preserve existing selected identities; no approval/reselection"}
    write(output / "hydration.json", hydration)
    components.append({"id": "hydration", "kind": "department-packet", "path": "hydration.json",
                       "sha256": sha(output / "hydration.json"), "required": True})
    write(output / "package.json", {"schema": "reel.production-package.v0.1", "work": "motioncraft-queue-study",
        "revision": "motioncraft-integrated-r1", "publication_scope": "internal-review", "components": components,
        "review_gates": [{"id": "creative-selection", "owner": "human-project-owner", "status": "pending"}]})


def hydrate(package_root: Path, profile: str, output: Path) -> None:
    if output.exists():
        raise FileExistsError(output)
    package = read(package_root / "package.json")
    if package["schema"] != "reel.production-package.v0.1":
        raise ValueError("unsupported package")
    components = {}
    for entry in package["components"]:
        source = inside(package_root, entry["path"])
        if entry["path"] in components or sha(source) != entry["sha256"]:
            raise ValueError("package inventory duplicate or hash mismatch")
        components[entry["path"]] = entry
    if "hydration.json" not in components:
        raise ValueError("hydration manifest is not inventoried")
    hydration = read(package_root / "hydration.json")
    if hydration["schema"] != "reel.motioncraft-study-hydration.v1" or not hydration["synthetic_only"]:
        raise ValueError("unsupported hydration manifest")
    spec = hydration["profiles"][profile]
    prefix = f"studies/{profile}/"
    inputs = set(spec["authoring_files"])
    for entry in spec["assets"]:
        source = inside(package_root, entry["path"])
        if entry["path"] not in components or sha(source) != entry["sha256"] or source.stat().st_size != entry["bytes"]:
            raise ValueError("hydration asset differs from inventory")
        if entry["cache_uri"] != f"cache://sha256/{entry['sha256']}":
            raise ValueError("hydration locator mismatch")
        inputs.add(entry["path"])
    for relative in inputs:
        if not relative.startswith(prefix) or relative not in components:
            raise ValueError("hydration input is outside the requested profile/inventory")
        inside(package_root, relative)
    output.mkdir(parents=True)
    for relative in sorted(inputs):
        target = output / relative[len(prefix):]
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(inside(package_root, relative), target)
    write(output / "hydration-receipt.json", {"schema": "reel.motioncraft-study-hydration-receipt.v1",
        "package_sha256": sha(package_root / "package.json"), "hydration_sha256": sha(package_root / "hydration.json"),
        "profile": profile, "inputs": [{"path": p[len(prefix):], "sha256": components[p]["sha256"]} for p in sorted(inputs)],
        "scope": "Clean authored inputs and selected cache objects only; compiled/rendered outputs must be reproduced"})


def verify_reproduction(package_root: Path, profile: str, clean: Path) -> None:
    reference = package_root / "studies" / profile
    receipt = read(clean / "hydration-receipt.json")
    if receipt["profile"] != profile or receipt["package_sha256"] != sha(package_root / "package.json"):
        raise ValueError("clean workspace is not bound to this package/profile")
    for entry in receipt["inputs"]:
        if sha(inside(clean, entry["path"])) != entry["sha256"]:
            raise ValueError("hydrated input changed before reproduction")
    compiled = {}
    required = {"build.json", "compile-receipt.json", "contract.json", "job.json", "production.json", "semantic-delivery.json"}
    if {p.name for p in (reference / "compiled-revised").glob("*.json")} != required:
        raise ValueError("reference compiled output set is incomplete or unsupported")
    for source in sorted((reference / "compiled-revised").glob("*.json")):
        actual = clean / "compiled-revised" / source.name
        if sha(source) != sha(actual):
            raise ValueError(f"recompiled plan/lineage differs: {source.name}")
        compiled[source.name] = sha(actual)
    stems = {}
    for name in ("D.wav", "E.wav", "M.wav", "mix.wav"):
        actual = clean / "render-revised" / name
        if sha(reference / "render-revised" / name) != sha(actual):
            raise ValueError(f"reproduced native stem differs: {name}")
        stems[name] = sha(actual)
    def decoded_frames(path: Path) -> list[str]:
        data = subprocess.check_output(["ffmpeg", "-v", "error", "-nostdin", "-i", str(path),
            "-map", "0:v:0", "-an", "-f", "framemd5", "-"]).decode()
        return [line for line in data.splitlines() if line and not line.startswith("#")]
    expected = decoded_frames(reference / "render-revised/picture.mkv")
    actual = decoded_frames(clean / "render-revised/picture.mkv")
    spec = read(package_root / "hydration.json")["profiles"][profile]
    if len(actual) != spec["delivery_frames"] or actual != expected:
        raise ValueError("decoded reproduced picture frames differ")
    from PIL import Image
    evidence = read(reference / "render-revised/motioncraft/evidence.json")
    samples = []
    for frame in evidence["frames"]:
        name = frame["path"]
        with Image.open(reference / "render-revised/motioncraft" / name) as a, Image.open(clean / "render-revised/motioncraft" / name) as b:
            if a.size != b.size or a.convert("RGB").tobytes() != b.convert("RGB").tobytes():
                raise ValueError(f"reproduced review pixels differ: {name}")
        samples.append(frame["frame_index"])
    write(clean / "reproduction-check.json", {"schema": "reel.motioncraft-study-reproduction.v1",
        "package_sha256": receipt["package_sha256"], "profile": profile,
        "compiled_files_identical": compiled, "native_stems_identical": stems,
        "decoded_picture_frames_identical": len(actual), "review_sample_indices_identical": samples,
        "scope": "Same-runtime synthetic reproduction; decoded pixels/native samples, not encoded-video byte identity",
        "passed": True})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    packing = sub.add_parser("pack")
    packing.add_argument("output", type=Path)
    packing.add_argument("--landscape", type=Path, required=True)
    packing.add_argument("--portrait", type=Path, required=True)
    packing.add_argument("--caption-landscape", type=Path)
    packing.add_argument("--caption-portrait", type=Path)
    packing.add_argument("--producer", type=Path)
    hydrating = sub.add_parser("hydrate")
    hydrating.add_argument("package", type=Path)
    hydrating.add_argument("profile", choices=("landscape", "portrait", "caption-landscape", "caption-portrait"))
    hydrating.add_argument("output", type=Path)
    verifying = sub.add_parser("verify-reproduction")
    verifying.add_argument("package", type=Path)
    verifying.add_argument("profile", choices=("landscape", "portrait", "caption-landscape", "caption-portrait"))
    verifying.add_argument("clean", type=Path)
    args = parser.parse_args()
    if args.command == "pack":
        if bool(args.caption_landscape) != bool(args.caption_portrait):
            parser.error("supply both caption-reserved study roots")
        captions = {"landscape": args.caption_landscape.resolve(), "portrait": args.caption_portrait.resolve()} if args.caption_landscape else None
        pack({"landscape": args.landscape.resolve(), "portrait": args.portrait.resolve()}, args.output.resolve(), captions, args.producer)
    elif args.command == "hydrate":
        hydrate(args.package.resolve(), args.profile, args.output.resolve())
    else:
        verify_reproduction(args.package.resolve(), args.profile, args.clean.resolve())
