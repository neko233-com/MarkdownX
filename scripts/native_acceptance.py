"""Drive an isolated native MarkdownX with MyGo's per-app automation plug-in."""
from __future__ import annotations

import json
import os
import re
import secrets
import subprocess
import sys
import time
from pathlib import Path


def build_automation(root: Path, scratch: Path, env: dict[str, str]) -> tuple[Path, Path]:
    suffix = ".exe" if os.name == "nt" else ""
    driver = scratch / ("mygo-automation"+suffix)
    exe = scratch / ("MarkdownX-QA"+suffix)
    plugin = scratch / "qa_automation.go"
    plugin.write_text('package main\nimport _ "github.com/egoist/mygo/plugins/automation"\n', encoding="utf-8")
    settings = root / "internal" / "app" / "settings.go"
    settings_source = settings.read_text(encoding="utf-8")
    marker = "func settingsPath() string {"
    if settings_source.count(marker) != 1:
        raise ValueError("isolated settings hook moved")
    settings_overlay = scratch / "qa_settings.go"
    settings_overlay.write_text(settings_source.replace(
        marker, marker+'\n\tif path := os.Getenv("MDX_QA_CONFIG"); path != "" {\n\t\treturn path\n\t}',
    ), encoding="utf-8")
    templates = root / "internal" / "app" / "templates.go"
    templates_source = templates.read_text(encoding="utf-8")
    marker = "func templatesDir() string {"
    if templates_source.count(marker) != 1:
        raise ValueError("isolated templates hook moved")
    templates_overlay = scratch / "qa_templates.go"
    templates_overlay.write_text(templates_source.replace(
        marker, marker+'\n\tif path := os.Getenv("MDX_QA_CONFIG"); path != "" {\n\t\treturn filepath.Join(filepath.Dir(path), "templates")\n\t}',
    ), encoding="utf-8")
    overlay = scratch / "overlay.json"
    overlay.write_text(json.dumps({"Replace": {
        str(root / "cmd" / "markdownx" / "temporary_mdx_automation.go"): str(plugin),
        str(settings): str(settings_overlay),
        str(templates): str(templates_overlay),
    }}), encoding="utf-8")
    build = ["go", "build"]
    if os.name == "nt":
        build.append("-ldflags=-H=windowsgui")
    subprocess.run(["go", "build", "-o", str(driver), "github.com/egoist/mygo/plugins/automation/cmd/mygo-automation"], cwd=root, env=env, check=True)
    subprocess.run([*build, f"-overlay={overlay}", "-o", str(exe), "./cmd/markdownx"], cwd=root, env=env, check=True)
    return exe, driver


class NativeWindow:
    def __init__(self, driver: Path, env: dict[str, str]):
        self.driver, self.env = driver, env
        self.mod = "Cmd" if sys.platform == "darwin" else "Ctrl"

    def invoke(self, *args: str, window: str | None = None, strict: bool = True) -> str:
        command = [str(self.driver), *args]
        if window is not None:
            command.extend(("--window", window))
        result = subprocess.run(command, env=self.env, capture_output=True, encoding="utf-8", errors="replace", timeout=15)
        if strict and result.returncode:
            raise RuntimeError(f"MyGo {' '.join(args)}: {result.stderr.strip()}")
        return result.stdout if result.returncode == 0 else ""

    def snapshot(self, window="w1"):
        return json.loads(self.invoke("snapshot", "--json", window=window))

    def nodes(self, window="w1"):
        return self.snapshot(window)["nodes"]

    def key(self, *keys, window="w1"):
        self.invoke("key", *keys, "--settle", window=window)

    def prompt(self, text, window="w1"):
        self.invoke("type", text, "--settle", window=window)

    def screenshot(self, snapshots, name, window="w1"):
        self.invoke("screenshot", str(snapshots / f"native-{name}.png"), window=window)

    def click_node(self, role, label, window="w1"):
        node = next(n for n in self.nodes(window) if n["role"] == role and n.get("label") == label)
        r = node["bounds"]
        self.invoke("click", str(int(r["x"]+r["width"]/2)), str(int(r["y"]+r["height"]/2)), "--settle", window=window)


def has(nodes, role: str, label: str | None = None):
    return any(n["role"] == role and (label is None or label == n.get("label")) for n in nodes)


def windows_frame(pid: int) -> int:
    import ctypes
    from ctypes import wintypes

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    enum = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.IsWindowVisible.argtypes = (wintypes.HWND,)
    user32.GetWindowThreadProcessId.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.DWORD))
    user32.GetWindowLongW.argtypes = (wintypes.HWND, ctypes.c_int)
    user32.GetMenu.argtypes = (wintypes.HWND,)
    user32.GetMenu.restype = wintypes.HMENU
    user32.GetMenuItemCount.argtypes = (wintypes.HMENU,)
    user32.GetMenuStringW.argtypes = (wintypes.HMENU, wintypes.UINT, wintypes.LPWSTR, ctypes.c_int, wintypes.UINT)
    user32.EnumWindows.argtypes = (enum, wintypes.LPARAM)
    found = []

    @enum
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    assert len(found) == 1, f"unexpected top-level Windows frames: {len(found)}"
    hwnd = found[0]
    assert user32.GetWindowLongW(hwnd, -16) & 0x00C00000 == 0x00C00000, "missing native title bar"
    menu = user32.GetMenu(hwnd)
    labels = []
    for index in range(user32.GetMenuItemCount(menu)):
        label = ctypes.create_unicode_buffer(80)
        user32.GetMenuStringW(menu, index, label, len(label), 0x400)
        labels.append(label.value.replace("&", ""))
    assert "文件" in labels and "编辑" in labels and "视图" in labels, labels
    return hwnd


def capture_visible_frame(hwnd: int, path: Path) -> None:
    """Capture the visible compositor frame, including native menu and title."""
    import ctypes
    from ctypes import wintypes

    try:
        from PIL import ImageGrab
    except ImportError:
        return
    user32 = ctypes.WinDLL("user32")
    user32.GetForegroundWindow.restype = wintypes.HWND
    user32.WindowFromPoint.argtypes = (wintypes.POINT,)
    user32.WindowFromPoint.restype = wintypes.HWND
    user32.GetAncestor.argtypes = (wintypes.HWND, wintypes.UINT)
    user32.GetAncestor.restype = wintypes.HWND
    if user32.GetForegroundWindow() != hwnd:
        return
    rect = wintypes.RECT()
    get_bounds = ctypes.WinDLL("dwmapi").DwmGetWindowAttribute
    get_bounds.argtypes = (wintypes.HWND, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD)
    if get_bounds(hwnd, 9, ctypes.byref(rect), ctypes.sizeof(rect)) != 0:
        return
    corners = [(x, y) for x in (rect.left+12, rect.right-12) for y in (rect.top+12, rect.bottom-12)]
    if any(user32.GetAncestor(user32.WindowFromPoint(wintypes.POINT(x, y)), 2) != hwnd for x, y in corners):
        return
    image = ImageGrab.grab(bbox=(rect.left, rect.top, rect.right, rect.bottom), all_screens=True)
    if user32.GetForegroundWindow() == hwnd and image.getcolors(8) is None:
        image.save(path)


def native_acceptance(root: Path, scratch: Path, snapshots: Path, env: dict[str, str]) -> None:
    if sys.platform not in ("win32", "darwin"):
        raise RuntimeError("native acceptance requires a supported desktop")
    exe, driver = build_automation(root, scratch, env)
    workspace = scratch / "项目笔记"
    chapter = workspace / "操作手册" / "入门"
    chapter.mkdir(parents=True)
    for name in ("任务清单", "参考资料", "历史记录"):
        page = workspace / name / "概览.md"
        page.parent.mkdir()
        page.write_text("# 概览\n", encoding="utf-8")
    source, other = chapter / "2-阅读指南.md", chapter / "10-检查清单.md"
    document = """# 项目使用指南

## 开始阅读

在文件夹中整理项目资料，点选左边的文档，在右侧阅读并直接修改。

- **操作手册**记录主要流程。
- **任务清单**收集需要跟进的工作。
- **参考资料**存放项目约定和背景信息。

## 文件一览

| 文件 | 用途 |
| --- | --- |
| 阅读指南 | 工作区的使用说明 |
| 检查清单 | 阅读或编辑后的核对事项 |
| 概览 | 目录内容摘要 |

编辑示例。
"""
    source.write_bytes(document.replace("\n", "\r\n").encode("utf-8-sig"))
    other.write_text("# 检查清单\n\n确认当前文档。\n", encoding="utf-8")
    settings = scratch / "settings.json"
    settings.write_text(json.dumps({
        "root": str(workspace), "last": str(source), "recent": [str(source)],
        "workspace_on_launch": 3, "auto_save": False, "mode": 1, "tree_open_mode": 1,
    }, ensure_ascii=False), encoding="utf-8")
    socket = scratch / "ui.sock"
    run_env = {
        **env, "MYGO_AUTOMATION": str(socket), "MDX_QA_CONFIG": str(settings),
        "MYGO_AUTOMATION_SESSION": "mdx-qa-"+secrets.token_hex(6),
    }
    ui = NativeWindow(driver, run_env)

    def assert_workspace(filename, content):
        nodes = ui.nodes()
        assert sum(n["role"] == "tree" for n in nodes) == 1, "expected one file tree"
        assert has(nodes, "treeItem", filename), filename
        assert has(nodes, "tabList", "侧栏视图") and not has(nodes, "tabList", "打开的文档")
        assert not has(nodes, "button", "切换视图")
        matches = [n for n in nodes if n["role"] == "textField" and n.get("value") == content]
        assert len(matches) == 1, f"expected one editable copy of {content}"
        windows = ui.invoke("windows")
        assert re.search(r"w1.*"+re.escape(filename), windows) and "[content]" in windows, windows
        assert not re.search(r"\bw[2-9]\d*\s", windows), windows
        return nodes

    def assert_outline():
        snapshot = ui.snapshot()
        nodes = snapshot["nodes"]
        assert has(nodes, "list", "文档大纲") and not has(nodes, "tree") and not has(nodes, "dialog")
        assert has(nodes, "listItem", "开始阅读") and has(nodes, "listItem", "文件一览")
        assert has(nodes, "button", "切换到文件树")
        return snapshot

    log = scratch / "app.log"
    with log.open("w", encoding="utf-8") as output:
        app = subprocess.Popen([str(exe)], cwd=scratch, env=run_env, stdout=output, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic()+12
            while not re.search(r"w1.*2-阅读指南", ui.invoke("open", "--socket", str(socket), strict=False)):
                if app.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError(f"native MarkdownX did not start: {log.read_text(encoding='utf-8', errors='replace')}")
                time.sleep(0.12)

            nodes = assert_workspace(source.name, "编辑示例。")
            assert has(nodes, "treeItem", "操作手册") and has(nodes, "treeItem", "入门")
            entries = [n["label"] for n in nodes if n["role"] == "treeItem" and "label" in n]
            assert entries.index(source.name) < entries.index(other.name), entries
            ui.screenshot(snapshots, "workspace")
            if os.name == "nt":
                capture_visible_frame(windows_frame(app.pid), snapshots / "native-workspace-frame.png")

            ui.click_node("treeItem", "入门")
            assert not has(ui.nodes(), "treeItem", source.name)
            ui.click_node("treeItem", "入门")
            ui.click_node("treeItem", other.name)
            assert_workspace(other.name, "确认当前文档。")
            ui.key("Up", "Enter")
            assert_workspace(source.name, "编辑示例。")
            ui.click_node("button", "切换 Markdown 源码")
            assert has(ui.nodes(), "textField", "Markdown 源码")
            assert has(ui.nodes(), "treeItem", source.name)
            ui.screenshot(snapshots, "source-in-workspace")
            ui.key(f"{ui.mod}+/")
            nodes = assert_workspace(source.name, "编辑示例。")

            assert source.read_bytes() == document.replace("\n", "\r\n").encode("utf-8-sig")
            edit = next(n for n in nodes if n["role"] == "textField" and n.get("value") == "编辑示例。")
            r = edit["bounds"]
            ui.invoke("click", str(int(r["x"]+12)), str(int(r["y"]+r["height"]/2)), "--settle", window="w1")
            ui.key("End")
            ui.prompt("已验证")
            ui.key(f"{ui.mod}+S")
            expected = document.replace("编辑示例。", "编辑示例。已验证").replace("\n", "\r\n").encode("utf-8-sig")
            assert source.read_bytes() == expected
            ui.screenshot(snapshots, "edited-workspace")

            bottom_switch = next(n for n in ui.nodes() if n.get("label") == "切换到大纲")
            b = bottom_switch["bounds"]
            assert b["x"] < 40 and b["y"] > 500, "the sidebar-mode switch is outside the bottom-left corner"
            ui.click_node("button", "切换到大纲")
            assert_outline()
            ui.click_node("listItem", "文件一览")
            snapshot = assert_outline()
            assert any(n["role"] == "textField" and n.get("value") == "文件一览" and
                       n["id"] == snapshot.get("focus") for n in snapshot["nodes"]), "outline did not focus its target heading"
            ui.screenshot(snapshots, "outline")
            ui.click_node("button", "切换到文件树")
            assert_workspace(source.name, "编辑示例。已验证")
            ui.click_node("tab", "大纲")
            assert_outline()
            ui.click_node("tab", "文件")
            assert_workspace(source.name, "编辑示例。已验证")

            panel_mods = "Cmd+Ctrl" if sys.platform == "darwin" else "Ctrl+Shift"
            ui.key(f"{panel_mods}+1")
            assert_outline()
            ui.click_node("button", "隐藏侧栏")
            nodes = ui.nodes()
            assert not has(nodes, "list", "文档大纲") and has(nodes, "button", "显示侧栏")
            assert any(n["role"] == "textField" and n.get("value") == "编辑示例。已验证" for n in nodes)
            ui.screenshot(snapshots, "sidebar-hidden")
            ui.key(f"{ui.mod}+Shift+L")
            assert_outline()
            ui.key(f"{panel_mods}+3")
            assert_workspace(source.name, "编辑示例。已验证")
            assert has(ui.nodes(), "button", "隐藏侧栏")
        finally:
            try:
                ui.invoke("close", strict=False)
            finally:
                if app.poll() is None:
                    app.terminate()
                    try:
                        app.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        app.kill()
                        app.wait(timeout=4)
    print("Native desktop passed file-tree browsing, persistent outline and heading jumps, sidebar switching, source and byte-exact save.", flush=True)
