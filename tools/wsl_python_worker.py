"""Launch a staged Python worker through WSL without shell command strings.

Use as a staged Python tool under the existing Rust runner. WSL's native option
parser does not accept the runner's always-quoted CRT flags directly. A separate
Linux timeout bounds the worker even if the Windows launcher disappears.
"""
import argparse
from pathlib import Path
import re
import subprocess


def command(root, wsl, distribution, python, seconds, worker, arguments):
    root = root.resolve()
    for name in [wsl, worker]:
        path = Path(name)
        if path.is_absolute() or any(p in {"..", "."} for p in path.parts):
            raise ValueError("Expected staged workspace-relative worker/tool")
        if not (root / path).resolve().is_relative_to(root) or not (root / path).is_file():
            raise ValueError("Worker/tool escaped or is absent from workspace")
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,80}", distribution):
        raise ValueError("Invalid explicit WSL distribution")
    if not python.startswith("/") or "\0" in python or not 1 <= seconds <= 3600:
        raise ValueError("Explicit existing Linux interpreter and bounded timeout required")
    return [str(root / wsl), "-d", distribution, "--exec", "/usr/bin/timeout",
            "--kill-after=10", str(seconds), python, Path(worker).as_posix(), *arguments]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wsl-exe", required=True)
    parser.add_argument("--distribution", required=True)
    parser.add_argument("--python", required=True)
    parser.add_argument("--timeout-seconds", type=int, required=True)
    parser.add_argument("worker")
    parser.add_argument("arguments", nargs="*")
    args = parser.parse_args()
    root = Path.cwd()
    result = subprocess.run(command(root, args.wsl_exe, args.distribution, args.python,
                                    args.timeout_seconds, args.worker, args.arguments),
                            cwd=root, timeout=args.timeout_seconds + 15, check=False)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
