#!/usr/bin/env python3
"""Small, dependency-free CLI for agent-authored MarkdownX books.

Examples
--------
    python tools/agent/mdxb.py check examples/junior-physics.mdxb
    python tools/agent/mdxb.py pack books/physics --title "力与运动" -o physics.mdxb
    python tools/agent/mdxb.py unpack physics.mdxb books/physics
    python tools/agent/mdxb.py search books/physics 速度 --content
    python tools/agent/mdxb.py merge-annotations merged.json a.annotations.json b.annotations.json

The format is deliberately JSON: an agent can read and write it without a
build step, and `pack`/`unpack` keep a folder of Markdown chapters as the
source of truth.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

FORMAT = "markdownx.book"
VERSION = 2
MAX_ASSET = 16 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024
IMAGE = re.compile(r"!\[([^\]]*)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
HTML_IMAGE = re.compile(r'(<img\b[^>]*\bsrc=")([^"]+)(")', re.I)
ASSET_KEY = re.compile(r"^assets/[^\\:\x00-\x1f\x7f-\x9f]+$")


def die(message: str, code: int = 2) -> "NoReturn":
    print(message, file=sys.stderr)
    raise SystemExit(code)


def read_json(path: Path) -> dict:
    try:
        # utf-8-sig tolerates a leading BOM (e.g. files written by PowerShell).
        return json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        die(f"{path}: {exc}")


def slug(text: str) -> str:
    text = text.strip().lower()
    text = ''.join(c if c.isalnum() or c in '-_' else '-' for c in text)
    return re.sub(r'-+', '-', text).strip('-') or "book"


def validate_book(book: dict, source: str = "book") -> list[str]:
    errors: list[str] = []
    if book.get("format") != FORMAT:
        errors.append(f"{source}: format is {book.get('format')!r}, expected {FORMAT!r}")
    if book.get("version") not in (1, VERSION):
        errors.append(f"{source}: unsupported version {book.get('version')!r}")
    chapters = book.get("chapters")
    if not isinstance(chapters, list) or not chapters:
        errors.append(f"{source}: needs a non-empty chapters array")
        chapters = []
    seen: set[str] = set()
    for i, chapter in enumerate(chapters, 1):
        if not isinstance(chapter, dict):
            errors.append(f"{source}: chapter {i} is not an object")
            continue
        cid = chapter.get("id")
        if not isinstance(cid, str) or not cid.strip():
            errors.append(f"{source}: chapter {i} has no id")
        elif cid in seen:
            errors.append(f"{source}: duplicate chapter id {cid!r}")
        else:
            seen.add(cid)
        content = chapter.get("content")
        if not isinstance(content, str) or not content.strip():
            errors.append(f"{source}: chapter {cid or i!r} is empty")
        fmt = chapter.get("format")
        if fmt == "docxxml":
            try:
                ET.fromstring(f"<root>{content}</root>")
            except ET.ParseError as exc:
                errors.append(f"{source}: chapter {cid or i!r} is not valid DocxXML: {exc}")
        elif fmt not in ("markdown", "md"):
            errors.append(f"{source}: chapter {cid or i!r} has unknown format {fmt!r}")
        fmt_en = chapter.get("format_en", fmt)
        if "format_en" in chapter and book.get("version") != 2:
            errors.append(f"{source}: chapter {cid or i!r} needs version 2 for format_en")
        if fmt_en not in ("docxxml", "markdown", "md"):
            errors.append(f"{source}: chapter {cid or i!r} has unknown translation format {fmt_en!r}")
        elif fmt_en == "docxxml" and chapter.get("content_en"):
            try:
                ET.fromstring(f"<root>{chapter['content_en']}</root>")
            except ET.ParseError as exc:
                errors.append(f"{source}: chapter {cid or i!r} has invalid translated DocxXML: {exc}")
        title = chapter.get("title")
        if title is not None and not isinstance(title, dict):
            errors.append(f"{source}: chapter {cid or i!r} title must be an object")
    assets = book.get("assets") or {}
    if not isinstance(assets, dict):
        errors.append(f"{source}: assets must be an object")
        assets = {}
    total = 0
    for key, asset in assets.items():
        if not isinstance(key, str) or not ASSET_KEY.match(key) or ".." in key or "\\" in key or ":" in key:
            errors.append(f"{source}: unsafe asset key {key!r}")
            continue
        if not isinstance(asset, dict) or not isinstance(asset.get("data"), str):
            errors.append(f"{source}: asset {key!r} has no data")
            continue
        try:
            raw = base64.b64decode(asset["data"], validate=True)
        except Exception:
            errors.append(f"{source}: asset {key!r} is not valid base64")
            continue
        if len(raw) > MAX_ASSET:
            errors.append(f"{source}: asset {key!r} is over 16 MiB")
        total += len(raw)
    if total > MAX_TOTAL:
        errors.append(f"{source}: assets total {total} bytes, over 64 MiB")
    cover = book.get("cover") or ""
    if cover and not cover.startswith(("http://", "https://")) and cover not in assets:
        errors.append(f"{source}: cover {cover!r} is not an embedded asset")
    return errors


def cmd_check(args: argparse.Namespace) -> int:
    paths = [Path(p) for p in args.paths]
    if len(paths) == 1 and paths[0].is_dir():
        paths = sorted(paths[0].rglob("*.mdxb"))
    if not paths:
        die("no .mdxb files found")
    failed = 0
    for path in paths:
        book = read_json(path)
        errors = validate_book(book, str(path))
        if errors:
            failed += 1
            for error in errors:
                print(f"error: {error}", file=sys.stderr)
        else:
            chapters = book.get("chapters", [])
            print(f"ok {path} · {len(chapters)} chapter(s) · {len(book.get('assets') or {})} asset(s)")
    return 1 if failed else 0


def embed_assets(content: str, root: Path, assets: dict[str, dict]) -> str:
    def add(src: str) -> str:
        if src.startswith(("http://", "https://", "data:", "assets/")):
            return src
        path = (root / src).resolve()
        try:
            path.relative_to(root.resolve())
        except ValueError:
            return src
        if not path.is_file():
            return src
        raw = path.read_bytes()
        if len(raw) > MAX_ASSET:
            return src
        digest = hashlib.sha1(raw).hexdigest()[:8]
        key = f"assets/{digest}-{path.name}"
        assets[key] = {"mime": mime_of(path), "data": base64.b64encode(raw).decode("ascii")}
        return key

    content = IMAGE.sub(lambda m: f"![{m.group(1)}]({add(m.group(2))})", content)
    content = HTML_IMAGE.sub(lambda m: f"{m.group(1)}{add(m.group(2))}{m.group(3)}", content)
    return content


def collect_assets(root: Path, assets: dict[str, dict]) -> None:
    folder = root / "assets"
    if not folder.is_dir():
        return
    for path in sorted(p for p in folder.rglob("*") if p.is_file()):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            die(f"asset escapes the book folder: {path}")
        raw = path.read_bytes()
        if len(raw) > MAX_ASSET:
            continue
        key = f"assets/{path.relative_to(folder).as_posix()}"
        assets[key] = {"mime": mime_of(path), "data": base64.b64encode(raw).decode("ascii")}


def mime_of(path: Path) -> str:
    return {
        ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
        ".gif": "image/gif", ".webp": "image/webp", ".svg": "image/svg+xml",
        ".pdf": "application/pdf", ".mp4": "video/mp4", ".mp3": "audio/mpeg",
    }.get(path.suffix.lower(), "application/octet-stream")


def chapter_from_markdown(path: Path, root: Path, assets: dict[str, dict]) -> dict:
    text = path.read_text(encoding="utf-8")
    title = path.stem
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.startswith("# "):
            title = line[2:].strip() or title
            del lines[i]
            if i < len(lines) and not lines[i].strip():
                del lines[i]
            break
    return {
        "id": slug(path.stem),
        "title": {"zh": title, "en": title},
        "format": "markdown",
        "content": embed_assets("\n".join(lines).strip() + "\n", root, assets),
    }


def chapter_from_xml(path: Path, root: Path, assets: dict[str, dict]) -> dict:
    text = path.read_text(encoding="utf-8")
    title = path.stem
    try:
        node = ET.fromstring(f"<root>{text}</root>")
        for child in node:
            if child.tag == "title":
                title = "".join(child.itertext()).strip() or title
                break
    except ET.ParseError:
        pass
    return {
        "id": slug(path.stem),
        "title": {"zh": title, "en": title},
        "format": "docxxml",
        "content": embed_assets(text, root, assets),
    }


def chapter_sources(root: Path) -> list[Path]:
    """Markdown and DocxXML chapters, with a same-stem XML winning when fresh.

    The desktop export writes both `01-topic.md` and `01-topic.xml`; editing the
    Markdown makes it newer, so the XML only restores structure while untouched.
    """
    files = [p for p in root.rglob("*") if p.is_file() and ".git" not in p.parts
             and "node_modules" not in p.parts and not p.name.endswith((".en.md", ".en.xml", ".en.docxxml"))
             and p.suffix.lower() in (".md", ".xml", ".docxxml")]
    by_stem: dict[Path, Path] = {}
    for path in files:
        stem = path.with_suffix("")
        current = by_stem.get(stem)
        if current is None:
            by_stem[stem] = path
            continue
        # Prefer XML only if it is at least as new as its Markdown sibling.
        if path.suffix.lower() in (".xml", ".docxxml") and path.stat().st_mtime >= current.stat().st_mtime:
            by_stem[stem] = path
    return sorted(by_stem.values())


def cmd_pack(args: argparse.Namespace) -> int:
    root = Path(args.folder).resolve()
    if not root.is_dir():
        die(f"not a folder: {root}")
    files = chapter_sources(root)
    if args.glob:
        files = sorted(root.glob(args.glob))
    if not files:
        die(f"no Markdown or DocxXML chapters under {root}")
    assets: dict[str, dict] = {}
    collect_assets(root, assets)
    chapters = []
    meta_path = root / "book.json"
    meta = read_json(meta_path) if meta_path.is_file() else {}
    meta_chapters = meta.get("chapters", [])
    for key, mime in (meta.get("asset_mimes") or {}).items():
        if key in assets and isinstance(mime, str):
            assets[key]["mime"] = mime
    for path in files:
        if args.skip_readme and path.name.lower() == "readme.md":
            continue
        chapter = chapter_from_xml(path, root, assets) if path.suffix.lower() in (".xml", ".docxxml") \
            else chapter_from_markdown(path, root, assets)
        relative = path.relative_to(root).as_posix()
        kept = next((m for m in meta_chapters if isinstance(m, dict) and relative in (m.get("file"), m.get("source"))), None)
        if kept is None and len(chapters) < len(meta_chapters) and isinstance(meta_chapters[len(chapters)], dict):
            kept = meta_chapters[len(chapters)]
        if kept:
            if kept.get("id"):
                chapter["id"] = kept["id"]
            if kept.get("title"):
                chapter["title"] = kept["title"]
            if kept.get("content_en"):
                chapter["content_en"] = kept["content_en"]
        english_name = kept.get("content_en_file") if kept else None
        english = (root / english_name).resolve() if english_name else path.with_name(path.stem + ".en.md")
        try:
            english.relative_to(root)
        except ValueError:
            die(f"translation is outside the book folder: {english_name}")
        if english.is_file():
            chapter["content_en"] = english.read_text(encoding="utf-8")
            fmt_en = (kept.get("format_en") if kept else None) or ("docxxml" if english.suffix.lower() in (".xml", ".docxxml") else "markdown")
            if fmt_en != chapter["format"]:
                chapter["format_en"] = fmt_en
        chapters.append(chapter)
    if not chapters:
        die("no chapters after skipping README")
    original_title = meta.get("title") if isinstance(meta.get("title"), dict) else {}
    title = args.title or original_title.get("zh") or original_title.get("en") or root.name
    original_subtitle = meta.get("subtitle") if isinstance(meta.get("subtitle"), dict) else {}
    theme = dict(meta.get("theme") or {"accent": "#b05226", "background": "", "text": "", "font": "serif"})
    if args.accent:
        theme["accent"] = args.accent
    book = {
        "format": FORMAT,
        "version": 2 if any("format_en" in chapter for chapter in chapters) else 1,
        "id": args.id or meta.get("id") or slug(title),
        "title": {"zh": title, "en": args.title_en or original_title.get("en") or title},
        "subtitle": {"zh": args.subtitle if args.subtitle is not None else original_subtitle.get("zh", ""),
                     "en": args.subtitle_en if args.subtitle_en is not None else original_subtitle.get("en", args.subtitle or "")},
        "authors": args.author or meta.get("authors") or [],
        "languages": meta.get("languages") or ["zh", "en"],
        "default_language": args.lang or meta.get("default_language") or "zh",
        "cover": args.cover or meta.get("cover") or "",
        "theme": theme,
        "reading": meta.get("reading") or {"layout": "scroll", "max_width": 760, "show_toc": True},
        "chapters": chapters,
        "assets": assets,
        "created": 0,
        "modified": 0,
        "generator": "tools/agent/mdxb.py",
    }
    errors = validate_book(book, "new book")
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    out = Path(args.output).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(book, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"packed {out} · {len(chapters)} chapter(s) · {len(assets)} asset(s)")
    return 0


def cmd_unpack(args: argparse.Namespace) -> int:
    book = read_json(Path(args.book))
    errors = validate_book(book, args.book)
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    out = Path(args.folder)
    out.mkdir(parents=True, exist_ok=True)
    meta = {k: v for k, v in book.items() if k not in ("chapters", "assets")}
    meta["asset_mimes"] = {key: value.get("mime", "") for key, value in (book.get("assets") or {}).items()}
    meta["chapters"] = []
    for i, chapter in enumerate(book.get("chapters", []), 1):
        ext = "xml" if chapter.get("format") == "docxxml" else "md"
        name = f"{i:02d}-{slug(chapter.get('id') or str(i))}.{ext}"
        (out / name).write_text(chapter.get("content", ""), encoding="utf-8")
        item = {k: v for k, v in chapter.items() if k not in ("content", "content_en")}
        item["file"] = name
        if chapter.get("content_en"):
            en_ext = "xml" if chapter.get("format_en", chapter.get("format")) == "docxxml" else "md"
            en_name = f"{i:02d}-{slug(chapter.get('id') or str(i))}.en.{en_ext}"
            (out / en_name).write_text(chapter["content_en"], encoding="utf-8")
            item["content_en_file"] = en_name
            item["format_en"] = chapter.get("format_en", chapter.get("format"))
        meta["chapters"].append(item)
    (out / "book.json").write_text(json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for key, asset in (book.get("assets") or {}).items():
        path = out / key
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(base64.b64decode(asset["data"]))
    print(f"unpacked {len(book.get('chapters', []))} chapter(s) into {out}")
    return 0


def cmd_search(args: argparse.Namespace) -> int:
    root = Path(args.folder)
    query = args.query.lower()
    hits = []
    # Books unpack to `.md` (Markdown) and `.xml` (DocxXML) chapters; the
    # desktop also writes a parallel `.en.md` for the English text, which we
    # still search for content but never score on its filename.
    for path in sorted(p for p in root.rglob("*") if p.suffix.lower() in (".md", ".xml")):
        rel = str(path.relative_to(root)).replace("\\", "/")
        if rel.endswith(".en.md"):
            continue  # counted with its base chapter, not scored as a path
        score = 0
        if query in rel.lower():
            score += 3
        snippet = ""
        if args.content:
            text = path.read_text(encoding="utf-8", errors="ignore")
            pos = text.lower().find(query)
            if pos >= 0:
                score += 1
                start = max(0, pos - 36)
                snippet = " ".join(text[start:pos + len(query) + 48].split())
        if score:
            hits.append((score, rel, snippet))
    for score, rel, snippet in sorted(hits, reverse=True)[:50]:
        print(f"{score}\t{rel}" + (f"\t{snippet}" if snippet else ""))
    return 0 if hits else 1


def cmd_merge_annotations(args: argparse.Namespace) -> int:
    merged: dict | None = None
    for path in map(Path, args.inputs):
        data = read_json(path)
        if data.get("format") != "markdownx.annotations":
            die(f"{path}: not an annotations file")
        if merged is None:
            merged = data
            continue
        by_id = {a["id"]: a for a in merged.get("annotations", [])}
        for item in data.get("annotations", []):
            old = by_id.get(item["id"])
            if old is None or item.get("updated", 0) > old.get("updated", 0):
                by_id[item["id"]] = item
        merged["annotations"] = list(by_id.values())
    if merged is None:
        die("no inputs")
    Path(args.output).write_text(json.dumps(merged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"merged {len(merged.get('annotations', []))} annotation(s) into {args.output}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check", help="validate one book, or every .mdxb under a folder")
    check.add_argument("paths", nargs="+")
    check.set_defaults(func=cmd_check)

    pack = sub.add_parser("pack", help="pack a folder of Markdown chapters into one .mdxb")
    pack.add_argument("folder")
    pack.add_argument("-o", "--output", required=True)
    pack.add_argument("--title")
    pack.add_argument("--title-en")
    pack.add_argument("--subtitle")
    pack.add_argument("--subtitle-en")
    pack.add_argument("--author", action="append")
    pack.add_argument("--id")
    pack.add_argument("--lang")
    pack.add_argument("--accent")
    pack.add_argument("--cover")
    pack.add_argument("--glob")
    pack.add_argument("--skip-readme", action="store_true")
    pack.set_defaults(func=cmd_pack)

    unpack = sub.add_parser("unpack", help="write book.json, chapters and assets")
    unpack.add_argument("book")
    unpack.add_argument("folder")
    unpack.set_defaults(func=cmd_unpack)

    search = sub.add_parser("search", help="fuzzy path and optional content search")
    search.add_argument("folder")
    search.add_argument("query")
    search.add_argument("--content", action="store_true")
    search.set_defaults(func=cmd_search)

    merge = sub.add_parser("merge-annotations", help="merge annotation sidecars by newest updated")
    merge.add_argument("output")
    merge.add_argument("inputs", nargs="+")
    merge.set_defaults(func=cmd_merge_annotations)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
