#!/usr/bin/env python3
"""Remove repo-local test residue before a test run.

Only fixed, generated paths inside this checkout are touched. User libraries
(`MARKDOWNX_HOME`, `~/Documents/MarkdownX`), installed apps and the current
MSI are never targets. Run with --dry-run to inspect the list.
"""
from __future__ import annotations

import argparse
import os
import re
import shutil
import stat
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


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
    for parent in (ROOT, ROOT / "web", ROOT / "server", ROOT / "examples", ROOT / "scripts", ROOT / "tools"):
        if not parent.is_dir():
            continue
        for pycache in parent.glob("__pycache__"):
            found.append(pycache)
    dist = ROOT / "dist"
    if dist.is_dir():
        for pattern in ("*.log", "*.wixpdb", "*.wixobj"):
            found.extend(sorted(dist.glob(pattern)))
        manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
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


def _force_remove(func, path, _exc):
    """Windows marks some Git object files read-only; make them writable."""
    os.chmod(path, stat.S_IWRITE)
    func(path)


def clean(dry_run: bool = False) -> int:
    removed = 0
    kept = 0
    for path in targets():
        rel = path.relative_to(ROOT)
        if dry_run:
            print(f"{'dir' if path.is_dir() else 'file'} {rel}")
            continue
        try:
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
    args = parser.parse_args()
    return clean(args.dry_run)


if __name__ == "__main__":
    raise SystemExit(main())
