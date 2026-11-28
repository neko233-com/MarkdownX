"""Verify the Windows installer hooks with an isolated NSIS package and registry entry."""
from __future__ import annotations

import os
import shutil
import subprocess
import time
import uuid
from pathlib import Path


def find_makensis(env: dict[str, str]) -> str:
    if tool := shutil.which("makensis", path=env.get("PATH")):
        return tool
    candidates = [Path(env[key]) / "NSIS" / "makensis.exe"
                  for key in ("ProgramFiles", "ProgramFiles(x86)") if key in env]
    cache = Path(env["LOCALAPPDATA"]) / "mygo"
    candidates.extend(sorted(cache.glob("nsis-*/Bin/makensis.exe"), reverse=True))
    for path in candidates:
        if path.is_file():
            return str(path)
    raise RuntimeError("NSIS is needed for Windows installer acceptance (mygo build installs it on Windows)")


def windows_installer_acceptance(root: Path, scratch: Path, env: dict[str, str]) -> None:
    import winreg

    folder = scratch / "installer"
    folder.mkdir()
    default = folder / "Default"
    custom = folder / "自选 安装目录"
    key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\MarkdownX-QA-"+uuid.uuid4().hex
    hook = root / "scripts" / "windows-installer.nsh"
    license_dir = root / "resources" / "licenses"
    license_names = sorted(path.name for path in license_dir.iterdir() if path.is_file())
    hook_text = hook.read_text(encoding="utf-8")
    assert license_names and all(f'\\licenses\\{name}"' in hook_text for name in license_names)
    installer = folder / "fixture-setup.exe"
    payload = folder / "fixture.bin"
    payload.write_bytes(b"MarkdownX isolated installer fixture")
    script = folder / "fixture.nsi"
    script.write_text(f'''Unicode true
Name "MarkdownX QA"
OutFile "{installer}"
InstallDir "{folder / "NSIS default"}"
InstallDirRegKey HKCU "{key}" "InstallLocation"
RequestExecutionLevel user
!include "FileFunc.nsh"
Page directory
Page instfiles
UninstPage instfiles
!define MDX_DEFAULT_INSTALL_DIR "{default}"
!define MDX_UNINSTALL_KEY "{key}"
!include "{hook}"
Function .onInit
    !insertmacro customInit
FunctionEnd
Section
    SetOutPath "$INSTDIR"
    File /oname=MarkdownX.exe "{payload}"
    File /oname=.mygo-files "{payload}"
    SetOutPath "$INSTDIR\\licenses"
    File "{license_dir}\\*.txt"
    WriteUninstaller "$INSTDIR\\Uninstall.exe"
    WriteRegStr HKCU "{key}" "InstallLocation" "$INSTDIR"
SectionEnd
Section "Uninstall"
    DeleteRegKey HKCU "{key}"
    !insertmacro customUnInstall
    RMDir /r "$INSTDIR"
SectionEnd
''', encoding="utf-8")
    subprocess.run([find_makensis(env), "-V2", "-INPUTCHARSET", "UTF8", str(script)],
                   cwd=folder, env=env, check=True, timeout=30)

    def installed():
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as entry:
                return Path(winreg.QueryValueEx(entry, "InstallLocation")[0])
        except FileNotFoundError:
            return None

    def setup(path: Path | None = None, code: int = 0) -> None:
        command = subprocess.list2cmdline([str(installer), "/S"])
        if path is not None:
            command += " /D="+str(path)
        result = subprocess.run(command, cwd=folder, env=env, timeout=30)
        assert result.returncode == code, f"installer exit code {result.returncode}, expected {code}"

    def uninstall(path: Path) -> None:
        assert path in (default, custom) and path == installed(), path
        exe = path / "Uninstall.exe"
        subprocess.run([str(exe), "/S"], cwd=folder, env=env, check=True, timeout=15)
        deadline = time.monotonic()+10
        while installed() is not None or exe.exists():
            if time.monotonic() >= deadline:
                raise RuntimeError("isolated uninstaller did not finish")
            time.sleep(0.05)
        assert not (path / "MarkdownX.exe").exists()
        assert not (path / ".mygo-files").exists()
        assert all(not (path / "licenses" / name).exists() for name in license_names)

    try:
        setup()
        assert installed() == default and (default / "MarkdownX.exe").read_bytes() == payload.read_bytes()
        note = default / "licenses" / "留存.md"
        note.write_bytes(b"unrelated file")
        setup(custom, code=2)
        assert installed() == default and not custom.exists()
        uninstall(default)
        assert note.read_bytes() == b"unrelated file"

        setup(custom)
        assert installed() == custom
        setup()
        assert installed() == custom and (custom / "MarkdownX.exe").read_bytes() == payload.read_bytes()
        uninstall(custom)
        assert not custom.exists()
    finally:
        if (path := installed()) is not None:
            uninstall(path)
    print("Windows installer passed default path, Unicode custom path, in-place upgrade and selective uninstall.", flush=True)
