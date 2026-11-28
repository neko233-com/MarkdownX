#!/usr/bin/env python3
"""Run MarkdownX's Go checks and optional native desktop acceptance as one batch."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

from clean_test_artifacts import plain_entry
from native_acceptance import native_acceptance
from windows_installer_acceptance import windows_installer_acceptance

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_NAME = re.compile(r"[a-z0-9-]+\.png")


def publish_evidence(snapshots: Path, native: bool) -> None:
    target = ROOT / "dist" / "qa" / "desktop"
    resolved = target.resolve()
    if resolved != target or not resolved.is_relative_to(ROOT):
        raise ValueError(f"unexpected evidence path: {target}")
    target.mkdir(parents=True, exist_ok=True)
    shots = {}
    for image in sorted(snapshots.glob("*.png")):
        if not EVIDENCE_NAME.fullmatch(image.name):
            raise ValueError(f"unexpected screenshot name: {image.name}")
        with image.open("rb") as stream:
            signature, width, height = struct.unpack(">16sII", stream.read(24))
        if signature != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR" or min(width, height) < 420:
            raise ValueError(f"invalid screenshot geometry: {image.name} ({width}x{height})")
        shots[image.name] = {"width": width, "height": height, "sha256": hashlib.sha256(image.read_bytes()).hexdigest()}
    manifest = target / "screenshots.json"
    previous = {}
    if manifest.exists():
        if not plain_entry(manifest):
            raise ValueError(f"unexpected evidence link: {manifest}")
        previous = json.loads(manifest.read_text(encoding="utf-8")).get("screenshots", {})
    for name in shots:
        path = target / name
        if path.exists() and not plain_entry(path) or path.is_symlink():
            raise ValueError(f"unexpected evidence link: {path}")
    for name in shots:
        shutil.copyfile(snapshots / name, target / name)
    for name in previous.keys() - shots.keys():
        if EVIDENCE_NAME.fullmatch(name):
            path = target / name
            if path.is_file() and plain_entry(path):
                path.unlink()
    version = json.loads((ROOT / "mygo.json").read_text(encoding="utf-8-sig"))["version"]
    manifest.write_text(json.dumps({"version": version, "native": native, "screenshots": shots}, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")
    print(f"{len(shots)} screenshots in {target}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", action="store_true", help="verify the native folder workspace on the current desktop")
    args = parser.parse_args()
    env = {**os.environ, "CGO_ENABLED": "0", "PYTHONDONTWRITEBYTECODE": "1"}
    for variable in ("GOOS", "GOARCH"):
        env.pop(variable, None)

    def run(*command: str, extra: dict[str, str] | None = None, cwd: Path = ROOT) -> None:
        print(" ".join(command), flush=True)
        subprocess.run(command, cwd=cwd, env={**env, **(extra or {})}, check=True)

    with TemporaryDirectory(prefix="mdx-qa-") as temp:
        scratch = Path(temp).resolve()
        snapshots = scratch / "screenshots"
        snapshots.mkdir()
        run("git", "diff", "--check")
        run(sys.executable, "-B", "-m", "unittest", "test_clean_test_artifacts", cwd=ROOT / "scripts")
        run("go", "vet", "./...")
        run("go", "tool", "mygo", "vet", ".")
        run("go", "test", "./...", "-count=1", extra={"MDX_SNAPSHOT_DIR": str(snapshots)})
        for goos, goarch in (("windows", "amd64"), ("darwin", "arm64")):
            run("go", "build", "./...", extra={"GOOS": goos, "GOARCH": goarch})
        if args.native:
            native_acceptance(ROOT, scratch, snapshots, env)
            if sys.platform == "win32":
                windows_installer_acceptance(ROOT, scratch, env)
        publish_evidence(snapshots, args.native)


if __name__ == "__main__":
    main()
