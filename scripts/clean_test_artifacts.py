#!/usr/bin/env python3
"""Remove repo-local test residue before a test run.

Only fixed, generated paths inside this checkout are touched. User libraries
(`MARKDOWNX_HOME`, `~/Documents/MarkdownX`), installed apps and the current
MSI are never targets. Run with --dry-run to inspect the list.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
KEEP_RELEASES = 3


def installed_msi_versions() -> set[tuple[int, int, int]] | None:
    """Windows versions still installed; None when Windows MSI does not apply."""
    if os.name != "nt":
        return None
    import winreg
    found: set[tuple[int, int, int]] = set()
    key_path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
    for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(hive, key_path, 0, winreg.KEY_READ | view) as parent:
                    for i in range(winreg.QueryInfoKey(parent)[0]):
                        try:
                            with winreg.OpenKey(parent, winreg.EnumKey(parent, i)) as item:
                                name = winreg.QueryValueEx(item, "DisplayName")[0]
                                version = winreg.QueryValueEx(item, "DisplayVersion")[0]
                                match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", str(version))
                                if str(name).casefold() == "markdownx" and match:
                                    found.add(tuple(map(int, match.groups())))
                        except OSError:
                            continue
            except OSError:
                continue
    return found


def targets() -> list[Path]:
    found: list[Path] = []
    found.extend(sorted(ROOT.glob(".dsh-*")))
    for name in ("test-results", "playwright-report", ".pytest_cache"):
        found.append(ROOT / name)
    for parent in (ROOT, ROOT / "web", ROOT / "server", ROOT / "examples", ROOT / "scripts", ROOT / "tools", ROOT / "tools" / "harness", ROOT / "tools" / "agent"):
        if not parent.is_dir():
            continue
        for pycache in parent.glob("__pycache__"):
            found.append(pycache)
    dist = ROOT / "dist"
    if dist.is_dir():
        for pattern in ("*.log", "*.wixpdb", "*.wixobj"):
            found.extend(sorted(dist.glob(pattern)))
        # MSI cleanup applies only to legacy Rust checkouts.
        cargo = ROOT / "Cargo.toml"
        manifest = cargo.read_text(encoding="utf-8") if cargo.is_file() else ""
        match = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"', manifest, re.MULTILINE)
        if match:
            current = tuple(map(int, match.groups()))
            current_msi = dist / ("MarkdownX-" + ".".join(match.groups()) + "-x64.msi")
            installed = installed_msi_versions()
            # Keep the installed version until the new MSI is installed too.
            if current_msi.exists() and (installed is None or current in installed):
                for candidate in sorted(dist.glob("MarkdownX-*-x64.msi")):
                    version = re.fullmatch(r"MarkdownX-(\d+)\.(\d+)\.(\d+)-x64\.msi", candidate.name)
                    number = tuple(map(int, version.groups())) if version else None
                    if number and number < current and (installed is None or number not in installed):
                        found.append(candidate)
                        checksum = candidate.with_name(candidate.name + ".sha256")
                        if checksum.exists():
                            found.append(checksum)
    return list(dict.fromkeys(p for p in found if p.exists() and p != ROOT))


def release_version(name: str) -> tuple[int, int, int] | None:
    match = re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", name)
    return tuple(map(int, match.groups())) if match else None


def plain_entry(path: Path) -> bool:
    info = path.lstat()
    return not path.is_symlink() and not getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT


def verified_local_release(directory: Path) -> bool:
    """The directory must contain only the verified outputs of a MyGo build."""
    platform = directory / "windows-amd64"
    marker = platform / ".mygo-files"
    checksums = platform / "SHA256SUMS"
    installer = f"markdownx-setup-{directory.name}-amd64.exe"
    required = {"MarkdownX.exe", installer}
    try:
        if directory.resolve(strict=True) != directory or list(directory.iterdir()) != [platform]:
            return False
        if not plain_entry(directory) or not plain_entry(platform) or not platform.is_dir():
            return False
        if not plain_entry(marker) or not plain_entry(checksums) or not marker.is_file() or not checksums.is_file():
            return False
        if set(marker.read_text(encoding="utf-8").splitlines()) != {"MarkdownX.exe", "licenses"}:
            return False
        files: dict[str, Path] = {}
        for path in platform.iterdir():
            if not plain_entry(path):
                return False
            if path.name == "licenses" and path.is_dir():
                for license_path in path.iterdir():
                    if not plain_entry(license_path) or not license_path.is_file() or license_path.suffix != ".txt":
                        return False
                    files[f"licenses/{license_path.name}"] = license_path
            elif path.name in required and path.is_file():
                files[path.name] = path
            elif path not in (marker, checksums):
                return False
        if not required <= files.keys():
            return False
        manifest: dict[str, str] = {}
        for line in checksums.read_text(encoding="utf-8").splitlines():
            match = re.fullmatch(r"([0-9a-fA-F]{64})  (.+)", line)
            if match is None:
                return False
            digest, name = match.groups()
            name = name.replace("\\", "/")
            if name in manifest:
                return False
            manifest[name] = digest.lower()
        if manifest.keys() != files.keys():
            return False
        for name, path in files.items():
            with path.open("rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != manifest[name]:
                    return False
        return True
    except (OSError, UnicodeError):
        return False


def release_targets() -> list[Path]:
    release_dir = ROOT / "dist" / "release"
    try:
        version = json.loads((ROOT / "mygo.json").read_text(encoding="utf-8-sig"))["version"]
        newest = release_version(version)
        installed = installed_msi_versions()
        if newest is None or installed is None or newest not in installed:
            return []
        if release_dir.resolve(strict=True) != release_dir or not plain_entry(release_dir):
            return []
        if not verified_local_release(release_dir / version):
            return []
        older = []
        for directory in release_dir.iterdir():
            number = release_version(directory.name)
            if number is not None and number < newest and verified_local_release(directory):
                older.append((number, directory))
    except (OSError, ValueError, KeyError, TypeError):
        return []
    older.sort(reverse=True)
    return [directory for number, directory in older[KEEP_RELEASES-1:] if number not in installed]


def _force_remove(func, path, _exc):
    """Windows marks some Git object files read-only; make them writable."""
    os.chmod(path, stat.S_IWRITE)
    func(path)


def clean(dry_run: bool = False, *, releases: bool = False) -> int:
    removed = 0
    kept = 0
    for path in release_targets() if releases else targets():
        rel = path.relative_to(ROOT)
        if dry_run:
            print(f"{'dir' if path.is_dir() else 'file'} {rel}")
            continue
        try:
            if releases and not verified_local_release(path):
                print(f"kept {rel}: package changed after verification")
                kept += 1
                continue
            if path.is_symlink() or not path.is_dir():
                path.unlink()
            else:
                shutil.rmtree(path, onerror=_force_remove)
        except OSError as exc:
            # A file held open by the running process (e.g. the log this very
            # run writes to) cannot be removed on Windows. Skip it; the next run
            # will clean it once the handle is gone.
            print(f"kept {rel}: {exc}")
            kept += 1
            continue
        removed += 1
        print(f"removed {rel}")
    print(f"removed {removed} item(s)" + (f", kept {kept}" if kept else ""))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--prune-releases", action="store_true", help="prune verified local MyGo packages after installing the current version")
    args = parser.parse_args()
    return clean(args.dry_run, releases=args.prune_releases)


if __name__ == "__main__":
    raise SystemExit(main())
