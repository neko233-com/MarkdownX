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
import html
import json
import re
import struct
import sys
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import quote, unquote, unquote_to_bytes, urlsplit
from js_modules import may_reference_local_assets, module_references

FORMAT = "markdownx.book"
VERSION = 2
MAX_ASSET = 16 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024
ASSET_KEY = re.compile(r"^assets/[^\\:\x00-\x1f\x7f-\x9f]+$")


def module_identity(suffix: str) -> str:
    """Serialize a URL query/fragment as a browser does, then encode its identity."""
    if not suffix:
        return ''
    suffix = suffix.rstrip(''.join(chr(c) for c in range(33))).replace('\t', '').replace('\n', '').replace('\r', '')
    query, marker, fragment = suffix.partition('#')
    query_safe = "!$%&()*+,-./:;=?@[]\\^_`{|}~"
    fragment_safe = "!'#$%&()*+,-./:;=?@[]\\^_{|}~"
    normalized = ('?' + quote(query[1:], safe=query_safe) if query.startswith('?') else query)
    if marker:
        normalized += '#' + quote(fragment, safe=fragment_safe)
    return '#' + ''.join(f'%{byte:02X}' for byte in normalized.encode('utf-8')) if normalized else ''


def markdown_image_at(text: str, start: int) -> tuple[int, int, int] | None:
    """Return destination bounds and end of an inline image, matching the desktop parser."""
    if not text.startswith('![', start): return None
    i, depth = start + 2, 1
    while i < len(text):
        ch = text[i]
        if ch == '\\' and i + 1 < len(text): i += 2; continue
        if ch == '[': depth += 1
        elif ch == ']':
            depth -= 1
            if depth == 0: break
        i += 1
    if text[i:i + 2] != '](': return None
    i += 2
    while i < len(text) and text[i].isspace(): i += 1
    angle = i < len(text) and text[i] == '<'
    if angle: i += 1
    begin, depth = i, 0
    while i < len(text):
        ch = text[i]
        if ch == '\\' and i + 1 < len(text): i += 2; continue
        if angle:
            if ch == '>': break
        else:
            if ch == '(': depth += 1
            elif ch == ')':
                if depth == 0: break
                depth -= 1
            elif ch.isspace() and depth == 0: break
        i += 1
    end = i
    if begin == end or depth or (angle and text[i:i + 1] != '>'): return None
    if angle: i += 1
    while i < len(text) and text[i].isspace(): i += 1
    if text[i:i + 1] in ('"', "'"):
        quote_char = text[i]; i += 1
        while i < len(text) and text[i] != quote_char:
            i += 2 if text[i] == '\\' and i + 1 < len(text) else 1
        if i == len(text): return None
        i += 1
        while i < len(text) and text[i].isspace(): i += 1
    return (begin, end, i + 1) if text[i:i + 1] == ')' else None


def reference_image_at(text: str, start: int) -> tuple[str, str, int] | None:
    if not text.startswith('![', start): return None
    at, depth = start + 2, 1
    while at < len(text):
        ch = text[at]
        if ch == '\\' and at + 1 < len(text): at += 2; continue
        if ch == '[': depth += 1
        elif ch == ']':
            depth -= 1
            if not depth: break
        at += 1
    if at == len(text): return None
    alt = text[start + 2:at]
    at += 1
    if text[at:at + 1] == '(': return None
    if text[at:at + 1] != '[': return alt, alt, at
    begin = at + 1
    at = begin
    while at < len(text) and text[at] != ']':
        at += 2 if text[at] == '\\' and at + 1 < len(text) else 1
    return (alt, text[begin:at] or alt, at + 1) if at < len(text) else None


def reference_key(label: str) -> str:
    label = re.sub(r'''\\([!"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])''', r'\1', label)
    return ' '.join(label.split()).lower()


def reference_table_safe(destination: str) -> str:
    out, slashes = [], 0
    for ch in destination:
        if ch == '|' and slashes % 2 == 0: out.append('\\')
        out.append(ch)
        slashes = slashes + 1 if ch == '\\' else 0
    return ''.join(out)


def reference_definition(line: str) -> re.Match | None:
    match = re.match(r'^ {0,3}\[((?:\\.|[^\[\]\\]){1,999})\]:\s*(.*?)\s*$', line)
    if not match or match.group(1).startswith('^') or not reference_key(match.group(1)): return None
    probe = '![](' + match.group(2) + ')'
    image = markdown_image_at(probe, 0)
    return match if image and image[2] == len(probe) else None


def markdown_code_lines(lines: list[str]) -> set[int]:
    """Conservative indented-code detection, including list and footnote bodies."""
    protected: set[int] = set()
    containers: list[int] = []
    fence: tuple[str, int, bool] | None = None
    code = False
    blank = True
    quote_depth = 0
    for index, original in enumerate(lines):
        line = original.rstrip('\r\n')
        depth = 0
        while match := re.match(r'^ {0,3}>[ \t]?', line):
            line = line[match.end():]
            depth += 1
        if fence:
            if not fence[2]: protected.add(index)
            indent_text = re.match(r'^[ \t]*', line).group()
            indent = len(indent_text.expandtabs(4))
            base = containers[-1] if containers else 0
            logical = line[len(indent_text):]
            if depth == quote_depth and indent <= base + 3 and re.fullmatch(re.escape(fence[0]) + '{' + str(fence[1]) + r',}\s*', logical): fence = None
            blank = not line.strip()
            continue
        if depth != quote_depth:
            containers.clear(); code = False; blank = True; quote_depth = depth
        if not line.strip():
            blank = True
            continue
        indent_text = re.match(r'^[ \t]*', line).group()
        indent = len(indent_text.expandtabs(4))
        while containers and indent < containers[-1]: containers.pop()
        base = containers[-1] if containers else 0
        logical = line[len(indent_text):]
        # Top-level and list-contained definitions own a four-column continuation.
        if indent <= base + 3 and re.match(r'^\[\^[^\]\\]+\]:', logical):
            containers.append(indent + 4)
            code = False; blank = False
            continue
        marker = re.match(r'^(?:[-+*]|\d{1,9}[.)。])([ \t]+)', logical)
        if marker and indent <= base + 3:
            before = line[:len(indent_text) + marker.start(1)]
            whitespace = marker.group(1)
            width = len((before + whitespace).expandtabs(4)) - len(before.expandtabs(4))
            padding = whitespace if width <= 4 and logical[marker.end():] else whitespace[0]
            containers.append(len((before + padding).expandtabs(4)))
            remaining = logical[marker.start(1) + len(padding):]
            code = len(re.match(r'^[ \t]*', remaining).group().expandtabs(4)) >= 4
            if code: protected.add(index)
            blank = False
            continue
        opening = re.match(r'^(`{3,}|~{3,})(.*)$', logical) if indent <= base + 3 else None
        if opening:
            info = opening.group(2).strip().lower()
            special = not containers and not depth and info in ('mdxb-html', 'mdxb-canvas')
            fence = (opening.group(1)[0], len(opening.group(1)), special)
            if not special: protected.add(index)
            code = False; blank = False
            continue
        if indent >= base + 4 and (blank or code):
            protected.add(index); code = True
        else:
            code = False
        blank = False
    return protected


def math_span_at(text: str, start: int) -> int | None:
    if text[start:start + 1] != '$' or start and text[start - 1] == '$': return None
    slash = start
    while slash and text[slash - 1] == '\\': slash -= 1
    if (start - slash) % 2: return None
    for delim in ('$$', '$'):
        if not text.startswith(delim, start) or text[start + len(delim):start + len(delim) + 1] == '$': continue
        begin = start + len(delim)
        if begin >= len(text) or text[begin].isspace(): continue
        end = begin
        while end < len(text) and text[end] not in '\r\n':
            if text[end] == '\\' and end + 1 < len(text): end += 2; continue
            if text.startswith(delim, end): break
            end += 1
        if end >= len(text) or text[end] in '\r\n' or end == begin or text[end - 1].isspace(): continue
        after = text[end + len(delim):end + len(delim) + 1]
        if after == '$' or delim == '$' and after.isascii() and after.isdigit(): continue
        return end + len(delim)
    return None


def display_math_end(lines: list[str], start: int) -> int | None:
    opening = re.match(r'^\s*((?:>\s*)*)\$\$\s*$', lines[start])
    if not opening: return None
    depth = opening.group(1).count('>')
    for end in range(start + 1, len(lines)):
        match = re.match(r'^\s*((?:>\s*)*)\$\$\s*$', lines[end])
        if match and match.group(1).count('>') == depth:
            body = ''.join(lines[start + 1:end])
            return end + 1 if body.strip() else None
        if depth and lines[end].strip() and not re.match(r'^\s*(?:>\s*){' + str(depth) + r'}', lines[end]): return None
    return None


def markdown_math_spans(line: str, in_comment: bool = False) -> list[tuple[int, int]]:
    destinations = []
    for at, ch in enumerate(line):
        if ch == '!':
            found = markdown_image_at(line, at)
            if found: destinations.append((found[0], found[1]))
        elif ch == '[' and (not at or line[at - 1] != '!'):
            found = markdown_image_at('!' + line[at:], 0)
            if found: destinations.append((at + found[0] - 1, at + found[1] - 1))
    spans, at = [], 0
    while at < len(line):
        if in_comment:
            end = line.find('-->', at)
            if end < 0: break
            at = end + 3; in_comment = False; continue
        if line.startswith('<!--', at): in_comment = True; at += 4; continue
        if line[at] == '\\' and at + 1 < len(line): at += 2; continue
        if line[at] == '`':
            run = re.match(r'`+', line[at:]).group()
            end = line.find(run, at + len(run))
            at = at + len(run) if end < 0 else end + len(run)
            continue
        if line[at] == '<' and re.match(r'</?[A-Za-z]', line[at:]):
            raw = re.match(r'<(script|style|textarea|title)(?=[\s>])', line[at:], re.I)
            if raw:
                end = re.search(rf'</{raw.group(1)}\s*>', line[at + raw.end():], re.I)
                at = len(line) if not end else at + raw.end() + end.end()
                continue
            quote_char, end = None, at + 1
            while end < len(line):
                if quote_char and line[end] == quote_char: quote_char = None
                elif not quote_char and line[end] in "\"'": quote_char = line[end]
                elif not quote_char and line[end] == '>': end += 1; break
                end += 1
            at = end; continue
        if line[at] == '$' and not any(a <= at < b for a, b in destinations):
            end = math_span_at(line, at)
            if end is not None: spans.append((at, end)); at = end; continue
        at += 1
    return spans


def reference_masks(lines: list[str]) -> list[str]:
    masks, fence, comment, raw, math_until = [], None, False, None, 0
    code_lines = markdown_code_lines(lines)
    for index, line in enumerate(lines):
        if index < math_until:
            masks.append(' ' * len(line)); continue
        if index in code_lines:
            masks.append(' ' * len(line)); continue
        mask = list(line)
        def erase(start: int, end: int) -> None:
            mask[start:end] = ' ' * (end - start)
        match = re.match(r'^ {0,3}(`{3,}|~{3,})(.*)$', line)
        if fence:
            if match and match.group(1)[0] == fence[0] and len(match.group(1)) >= len(fence) and not match.group(2).strip(): fence = None
            masks.append(' ' * len(line)); continue
        if not comment and not raw and match:
            fence = match.group(1)
            masks.append(' ' * len(line)); continue
        if not comment and not raw:
            end = display_math_end(lines, index)
            if end is not None:
                math_until = end
                masks.append(' ' * len(line)); continue
        at = 0
        while at < len(line):
            if comment:
                close = line.find('-->', at)
                end = len(line) if close < 0 else close + 3
                erase(at, end); at = end
                if close >= 0: comment = False
                continue
            if raw:
                close = re.search(rf'</{raw}\s*>', line[at:], re.I)
                end = at + close.end() if close else len(line)
                erase(at, end); at = end
                if close: raw = None
                continue
            if line.startswith('<!--', at): comment = True; continue
            if line[at] == '\\' and at + 1 < len(line): erase(at, at + 2); at += 2; continue
            if line[at] == '`':
                run = re.match(r'`+', line[at:]).group()
                close = line.find(run, at + len(run))
                end = at + len(run) if close < 0 else close + len(run)
                erase(at, end); at = end; continue
            math_end = math_span_at(line, at)
            if math_end is not None: erase(at, math_end); at = math_end; continue
            if line[at] == '<':
                tag = re.match(r'<(script|style|textarea|title|video|audio|canvas|model-viewer)(?=[\s>])', line[at:], re.I)
                if tag: raw = tag.group(1); continue
                if re.match(r'</?[A-Za-z]', line[at:]):
                    quote_char, end = None, at + 1
                    while end < len(line):
                        ch = line[end]
                        if quote_char and ch == quote_char: quote_char = None
                        elif not quote_char and ch in "\"'": quote_char = ch
                        elif not quote_char and ch == '>': end += 1; break
                        end += 1
                    erase(at, end); at = end; continue
            at += 1
        masks.append(''.join(mask))
    return masks


def media_div_end(lines: list[str], start: int, accept_line=None) -> int | None:
    """A balanced div containing media, excluding comments and raw script text."""
    parts, size = [], 0
    for at in range(start, min(len(lines), start + 1024)):
        if accept_line and not accept_line(at): break
        size += len(lines[at]) + 1
        if size > 256 * 1024: break
        parts.append(lines[at].rstrip('\r\n'))
    source = '\n'.join(parts).translate(str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'))
    at, depth, media, raw = 0, 0, False, None
    while at < len(source):
        if not raw and at and source[at - 1] == '\n' and re.match(r'^\s*(`{3,}|~{3,})', source[at:].split('\n', 1)[0]): return None
        if raw:
            close = '</' + raw
            while True:
                at = source.find(close, at)
                if at < 0: return None
                end = at + len(close)
                if end < len(source) and (source[end] == '>' or source[end].isspace()): break
                at = end
            raw = None
        if source[at] != '<': at += 1; continue
        if source.startswith('<!--', at):
            end = source.find('-->', at + 4)
            if end < 0: return None
            at = end + 3; continue
        p = at + 1
        closing = source[p:p + 1] == '/'
        if closing: p += 1
        name = re.match(r'[a-z0-9:-]+', source[p:])
        if not name: at += 1; continue
        tag = name.group(); p += len(tag); quote = None
        while p < len(source):
            ch = source[p]
            if quote and ch == quote: quote = None
            elif not quote and ch in "\"'": quote = ch
            elif not quote and ch == '>': break
            p += 1
        if p == len(source): return None
        self_closing = source[at:p].rstrip().endswith('/')
        if tag == 'div':
            if closing:
                depth -= 1
                if depth < 0: return None
                if depth == 0:
                    tail = source[p + 1:].split('\n', 1)[0].strip()
                    if not media or tail and not tail.startswith(('<script', '<style')): return None
                    return start + source[:p + 1].count('\n') + 1
            elif not self_closing: depth += 1
        elif not closing and depth > 0:
            if tag in ('video', 'audio', 'canvas', 'model-viewer'): media = True
            if tag in ('script', 'style', 'textarea', 'title') and not self_closing: raw = tag
        at = p + 1
    return None


def expand_reference_images(content: str) -> str:
    if '![' not in content or ']:' not in content: return content
    lines = content.split('\n')
    masks = reference_masks(lines)
    definitions, def_lines = {}, set()
    for i, line in enumerate(lines):
        match = reference_definition(line)
        if not match or masks[i][line.index('[')] != '[': continue
        key = reference_key(match.group(1))
        if not key: continue
        definitions.setdefault(key, match.group(2))
        def_lines.add(i)
    if not definitions: return content
    for i, line in enumerate(lines):
        if i in def_lines:
            continue
        edits, at = [], 0
        while at < len(line):
            if masks[i][at] != '!': at += 1; continue
            direct = markdown_image_at(line, at)
            if direct: at = direct[2]; continue
            image = reference_image_at(line, at)
            destination = definitions.get(reference_key(image[1])) if image else None
            if not destination: at += 1; continue
            edits.append((at, image[2], '![' + image[0] + '](' + reference_table_safe(destination) + ')'))
            at = image[2]
        for start, end, value in reversed(edits): line = line[:start] + value + line[end:]
        lines[i] = line
    return '\n'.join(lines)


def is_local_pdf(value: str) -> bool:
    value = html.unescape(value)
    if value.startswith('<') and value.endswith('>'): value = value[1:-1]
    if re.search(r'%(?![0-9a-fA-F]{2})', value): return False
    try:
        parsed = urlsplit(value)
        path = unquote_to_bytes(parsed.path).decode('utf-8')
    except (UnicodeDecodeError, ValueError): return False
    if parsed.scheme or parsed.netloc or parsed.query or not path or path.startswith('/') or any(ord(ch) < 32 or ch in '\\:' for ch in path): return False
    if not path.lower().endswith('.pdf'): return False
    if parsed.fragment:
        match = re.fullmatch(r'page=([0-9]+)', parsed.fragment)
        if not match or not 0 < int(match.group(1)) <= 2147483647: return False
    return True


def expand_reference_pdf_links(content: str) -> str:
    if ']:' not in content: return content
    lines = content.split('\n')
    masks = reference_masks(lines)
    definitions, def_lines = {}, set()
    for i, line in enumerate(lines):
        match = reference_definition(line)
        if not match or masks[i][line.index('[')] != '[': continue
        probe = '![](' + match.group(2) + ')'
        parsed = markdown_image_at(probe, 0)
        if parsed and is_local_pdf(probe[parsed[0]:parsed[1]]):
            definitions.setdefault(reference_key(match.group(1)), match.group(2))
            def_lines.add(i)
    for i, line in enumerate(lines):
        if i in def_lines: continue
        edits, at = [], 0
        while at < len(line):
            if masks[i][at] != '[' or at and line[at - 1] == '!': at += 1; continue
            probe = '!' + line[at:]
            direct = markdown_image_at(probe, 0)
            if direct: at += direct[2] - 1; continue
            link = reference_image_at(probe, 0)
            destination = definitions.get(reference_key(link[1])) if link else None
            if not destination: at += 1; continue
            end = at + link[2] - 1
            edits.append((at, end, '[' + link[0] + '](' + reference_table_safe(destination) + ')'))
            at = end
        for start, end, value in reversed(edits): line = line[:start] + value + line[end:]
        lines[i] = line
    return '\n'.join(lines)


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
    paths: dict[str, int] = {}
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
        for key in ('source_path', 'source_en_path'):
            value = chapter.get(key)
            if value is None: continue
            if not isinstance(value, str) or not value or len(value.encode('utf-8')) > 8192 or value.startswith('/') or any(ch in value for ch in '\\:') or any(ord(ch) < 32 or 127 <= ord(ch) < 160 for ch in value) or any(part in ('', '.', '..') for part in value.split('/')):
                errors.append(f"{source}: unsafe chapter source path {value!r}")
            elif value in paths and paths[value] != i:
                errors.append(f"{source}: duplicate chapter source path {value!r}")
            else:
                paths[value] = i
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


def css_references(source: str) -> list[tuple[int, int, str, bool, bool]]:
    """Find CSS URL tokens and @import strings, skipping comments and other strings."""
    def escape(i: int) -> tuple[str, int]:
        i += 1
        if i >= len(source): return '\ufffd', i
        if source[i] in '\r\n\f':
            return '', i + (2 if source[i:i + 2] == '\r\n' else 1)
        match = re.match(r'[0-9a-fA-F]{1,6}', source[i:])
        if match:
            value = int(match.group(), 16)
            i += len(match.group())
            if i < len(source) and source[i].isspace():
                i += 2 if source[i:i + 2] == '\r\n' else 1
            return chr(value) if 0 < value <= 0x10ffff and not 0xd800 <= value <= 0xdfff else '\ufffd', i
        return source[i], i + 1

    def string(i: int) -> tuple[str, int] | None:
        delim = source[i]
        i += 1
        result = []
        while i < len(source):
            if source[i] == delim: return ''.join(result), i + 1
            if source[i] in '\r\n\f': return None
            if source[i] == '\\':
                value, i = escape(i)
                result.append(value)
            else:
                result.append(source[i])
                i += 1
        return None

    def ident(i: int) -> tuple[str, int]:
        result = []
        while i < len(source):
            c = source[i]
            if c == '\\':
                value, i = escape(i)
                result.append(value)
            elif c.isalnum() or c in '-_' or ord(c) >= 128:
                result.append(c)
                i += 1
            else: break
        return ''.join(result), i

    result = []
    pending_import = False
    i = 0
    while i < len(source):
        if source.startswith('/*', i):
            end = source.find('*/', i + 2)
            i = len(source) if end < 0 else end + 2
            continue
        if source[i].isspace():
            i += 1
            continue
        start = i
        if source[i] == '@':
            name, i = ident(i + 1)
            pending_import = name.lower() == 'import'
            continue
        if source[i] in "\"'":
            parsed = string(i)
            if parsed is None: break
            value, i = parsed
            if pending_import: result.append((start, i, value, True, False))
            pending_import = False
            continue
        if source[i].isalpha() or source[i] in '-_\\' or ord(source[i]) >= 128:
            name, i = ident(i)
            if name.lower() == 'url' and i < len(source) and source[i] == '(':
                i += 1
                while i < len(source) and source[i].isspace(): i += 1
                if i < len(source) and source[i] in "\"'":
                    parsed = string(i)
                    if parsed is None: break
                    value, i = parsed
                    while i < len(source) and source[i].isspace(): i += 1
                    if i >= len(source) or source[i] != ')': pending_import = False; continue
                    i += 1
                else:
                    chars = []
                    while i < len(source) and source[i] != ')':
                        if source[i] == '\\':
                            value, i = escape(i)
                            chars.append(value)
                        else:
                            chars.append(source[i])
                            i += 1
                    if i >= len(source): break
                    i += 1
                    value = ''.join(chars).strip()
                result.append((start, i, value, pending_import, True))
            pending_import = False
            continue
        pending_import = False
        i += 1
    return result


def srcset_urls(value: str):
    """Yield URL spans; commas within data URLs are part of the URL."""
    pos = 0
    while pos < len(value):
        while pos < len(value) and (value[pos] in ' \t\n\r\f' or value[pos] == ','): pos += 1
        start = pos
        while pos < len(value) and value[pos] not in ' \t\n\r\f': pos += 1
        end = pos
        while end > start and value[end - 1] == ',': end -= 1
        if start < end: yield start, end
        if end != pos: continue
        depth = 0
        while pos < len(value):
            ch = value[pos]
            pos += 1
            if ch == '(': depth += 1
            elif ch == ')': depth = max(0, depth - 1)
            elif ch == ',' and depth == 0: break


def embed_assets(content: str, root: Path, assets: dict[str, dict], base: Path, *, markdown: bool) -> str:
    if markdown: content = expand_reference_pdf_links(expand_reference_images(content))
    root = root.resolve()
    loading: set[Path] = set()
    inline_total = 0
    module_cache: dict[tuple[Path, bool], str] = {}
    def add(src: str, *, xml: bool = False, source_base: Path | None = None, stylesheet: bool = False, module: bool = False, javascript: bool = False, media_time: bool = False) -> str:
        javascript = javascript or module
        current_base = source_base or base
        original = src
        src = html.unescape(src) if xml else src
        angled = src.startswith('<') and src.endswith('>')
        target = src[1:-1] if angled else src
        if target.lower().startswith(("http://", "https://", "data:", "blob:", "#")):
            return src
        parsed = urlsplit(target)
        fragment = parsed.fragment
        valid_time = media_time and re.fullmatch(r't=(?:npt:)?(?=[0-9:.,]*[0-9])[0-9:.,]+', fragment) and fragment.count(',') <= 1
        svg_fragment = bool(fragment and not javascript and not stylesheet and unquote(parsed.path).lower().endswith('.svg'))
        pdf_fragment = bool(fragment and not javascript and not stylesheet and is_local_pdf(target))
        query_at = target.find('?') if javascript else -1
        script_suffix = target[query_at:] if query_at >= 0 else ''
        encoded_identity = bool(module and fragment[:3].lower() == '%3f' and re.fullmatch(r'(?:%[0-9a-fA-F]{2})+', fragment))
        if module and script_suffix:
            identity = module_identity(script_suffix)
        elif encoded_identity:
            try:
                identity = module_identity(unquote_to_bytes(fragment).decode('utf-8'))
            except UnicodeDecodeError:
                identity = '#' + fragment
        else:
            identity = ''
        if parsed.scheme or parsed.netloc or fragment and not valid_time and not svg_fragment and not pdf_fragment and not script_suffix and not encoded_identity:
            die(f"unsupported local asset reference {target!r} in {current_base}")
        path = (current_base / unquote(parsed.path)).resolve()
        try:
            path.relative_to(root)
        except ValueError:
            die(f"image is outside the book folder: {target}")
        # Existing book sources can refer to their root-level asset map. A
        # chapter-local file wins; explicit ./ never falls back to the root.
        key = unquote(parsed.path)
        if not path.is_file() and not key.startswith('./') and key.startswith('assets/') and key in assets:
            path = (root / key).resolve()
            if not path.is_relative_to(root):
                die(f"image is outside the book folder: {target}")
        if not path.is_file():
            die(f"asset does not exist: {target} (relative to {current_base})")
        try:
            root_key = path.relative_to(root).as_posix()
        except ValueError:
            die(f"image is outside the book folder: {target}")
        if key.removeprefix('./') == root_key and root_key in assets and path.suffix.lower() not in ('.gltf', '.glb') and not stylesheet and not javascript:
            return src if xml else original
        if javascript and (path, module) in module_cache:
            key = module_cache[path, module]
            return key + identity if xml else './' + quote(key, safe='/') + identity
        if path.stat().st_size > MAX_ASSET:
            die(f"image is over 16 MiB: {path}")
        raw = path.read_bytes()
        if len(raw) > MAX_ASSET:
            die(f"image is over 16 MiB: {path}")
        if stylesheet:
            if path in loading:
                die(f"circular stylesheet dependency: {path}")
            if len(loading) >= 16:
                die(f"stylesheet imports are too deep: {path}")
            loading.add(path)
            try:
                raw = bundle_css(path, raw)
            finally:
                loading.remove(path)
            if len(raw) > MAX_ASSET:
                die(f"stylesheet is over 16 MiB after packaging: {path}")
        elif javascript and (module or any(word in raw for word in (b'import', b'fetch', b'Image', b'Audio', b'createElement'))):
            if path in loading: die(f"circular JavaScript imports are not supported: {path}")
            if len(loading) >= 16: die(f"JavaScript imports are too deep: {path}")
            loading.add(path)
            try:
                raw = bundle_js(raw, path.parent)
                if module:
                    module_path_identity = ''.join(f'%{byte:02X}' for byte in path.relative_to(root).as_posix().encode('utf-8'))
                    raw += f'\n/* markdownx-module:{module_path_identity} */\n'.encode('ascii')
            finally:
                loading.remove(path)
            if len(raw) > MAX_ASSET: die(f"script is over 16 MiB after packaging: {path}")
        elif path.suffix.lower() in ('.gltf', '.glb'):
            if path in loading:
                die(f"circular model dependency: {path}")
            loading.add(path)
            try:
                raw = bundle_model(path, raw)
            finally:
                loading.remove(path)
            if len(raw) > MAX_ASSET:
                die(f"model is over 16 MiB after packaging: {path}")
        digest = hashlib.sha256(raw).hexdigest()
        ext = path.suffix.lower() if re.fullmatch(r'\.[a-z0-9]{1,12}', path.suffix.lower()) else '.bin'
        key = f"assets/{digest}{ext}"
        assets[key] = {"mime": "text/css" if stylesheet else "text/javascript" if module else mime_of(path), "data": base64.b64encode(raw).decode("ascii")}
        if javascript: module_cache[path, module] = key
        suffix = '#' + quote(fragment, safe="!$&'()*+,-./:;=?@_~%") if valid_time or svg_fragment or pdf_fragment else identity
        return key + suffix if xml else './' + quote(key, safe='/') + suffix

    def bundle_js(raw: bytes, source_base: Path) -> bytes:
        try:
            source = raw.decode('utf-8')
            references = module_references(source)
        except (UnicodeDecodeError, ValueError) as error:
            die(f"invalid JavaScript module: {error}")
        changes, output_bytes = [], len(raw)
        for start, end, value, kind in references:
            if kind == 'module' and not value.startswith(('./', '../')): continue
            if kind != 'module' and (not value or value.startswith(('/', '#', '?')) or ':' in value): continue
            parts = urlsplit(value)
            key = add(parts.path, xml=True, source_base=base if kind in ('fetch', 'dom') else source_base, module=kind == 'module')
            asset = assets[key]
            data = f"data:{asset['mime']};base64,{asset['data']}"
            suffix = value[len(parts.path):]
            if kind != 'module':
                if parts.fragment: data += '#' + parts.fragment
            elif suffix: data += module_identity(suffix)
            replacement = json.dumps(data, ensure_ascii=True)
            output_bytes += len(replacement.encode('utf-8')) - len(source[start:end].encode('utf-8'))
            if output_bytes > MAX_ASSET: die('JavaScript is over 16 MiB after packaging')
            changes.append((start, end, replacement))
        for start, end, replacement in reversed(changes): source = source[:start] + replacement + source[end:]
        return source.encode('utf-8')

    def bundle_css(path: Path, raw: bytes) -> bytes:
        try:
            source = raw.decode('utf-8')
        except UnicodeDecodeError:
            die(f"stylesheet is not UTF-8: {path}")
        changes = []
        output_bytes = len(raw)
        for start, end, value, imported, function in css_references(source):
            target = value.strip()
            if not target or target.startswith(('#', '//')) or target.lower().startswith(('http:', 'https:', 'data:', 'blob:')):
                continue
            parts = urlsplit(target)
            if parts.scheme or parts.netloc or not parts.path:
                die(f"unsupported stylesheet dependency: {target!r}")
            key = add(parts.path, xml=True, source_base=path.parent, stylesheet=imported)
            key = unquote(key.removeprefix('./'))
            if key not in assets:
                die(f"stylesheet dependency could not be embedded: {target!r}")
            asset = assets[key]
            url = f"data:{asset['mime']};base64,{asset['data']}"
            if parts.fragment:
                url += '#' + quote(parts.fragment, safe="!$&'()*+,-./:;=?@_~%")
            replacement = f'url("{url}")' if function else f'"{url}"'
            output_bytes += len(replacement.encode('utf-8')) - len(source[start:end].encode('utf-8'))
            if output_bytes > MAX_ASSET:
                die(f"stylesheet is over 16 MiB after packaging: {path}")
            changes.append((start, end, replacement))
        for start, end, replacement in reversed(changes):
            source = source[:start] + replacement + source[end:]
        return source.encode('utf-8')

    def bundle_model(path: Path, data: bytes) -> bytes:
        def rewrite_json(value: dict) -> bool:
            if not isinstance(value, dict):
                die(f"invalid glTF JSON: {path}")
            changed = False
            for group in ('buffers', 'images'):
                items = value.get(group, [])
                if not isinstance(items, list):
                    die(f"invalid glTF {group}: {path}")
                for item in items:
                    if not isinstance(item, dict):
                        die(f"invalid glTF {group} entry: {path}")
                    if 'uri' not in item:
                        continue
                    uri = item['uri']
                    if not isinstance(uri, str):
                        die(f"invalid glTF dependency URI: {path}")
                    if uri.startswith('data:'): continue
                    if not uri or any(c in uri for c in ':?#\\'):
                        die(f"unsupported glTF dependency: {uri!r}")
                    key = add(uri, xml=True, source_base=path.parent)
                    if not key.startswith('assets/'):
                        die(f"glTF dependency could not be embedded: {uri!r}")
                    # Packaged model and dependencies live in assets/ together.
                    item['uri'] = './' + key.removeprefix('assets/')
                    changed = True
            return changed
        try:
            if path.suffix.lower() == '.gltf':
                value = json.loads(data)
                return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8') if rewrite_json(value) else data
            if len(data) < 20 or data[:4] != b'glTF':
                die(f"invalid GLB: {path}")
            version, declared = struct.unpack_from('<II', data, 4)
            if version != 2 or declared != len(data):
                die(f"invalid GLB header: {path}")
            chunks, at, saw_json, changed = [], 12, False, False
            while at < len(data):
                if len(data) - at < 8:
                    die(f"invalid GLB chunk: {path}")
                size, kind = struct.unpack_from('<II', data, at)
                at += 8
                if size % 4 or size > len(data) - at:
                    die(f"invalid GLB chunk length: {path}")
                chunk = data[at:at + size]
                if (not saw_json and kind != 0x4e4f534a) or (saw_json and kind == 0x4e4f534a):
                    die(f"invalid GLB JSON chunk order: {path}")
                if kind == 0x4e4f534a and not saw_json:
                    saw_json = True
                    value = json.loads(chunk)
                    if rewrite_json(value):
                        changed = True
                        chunk = json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8')
                        chunk += b' ' * (-len(chunk) % 4)
                chunks.append((kind, chunk))
                at += size
            if not saw_json:
                die(f"GLB has no JSON chunk: {path}")
            if not changed:
                return data
            total = 12 + sum(8 + len(chunk) for _, chunk in chunks)
            return b'glTF' + struct.pack('<II', 2, total) + b''.join(struct.pack('<II', len(chunk), kind) + chunk for kind, chunk in chunks)
        except (ValueError, TypeError, UnicodeDecodeError, struct.error) as error:
            die(f"invalid glTF model {path}: {error}")

    def rewrite(line: str) -> str:
        # Do not treat Markdown-looking text inside HTML attributes or
        # comments as an image reference.
        protected, at = [], 0
        while at < len(line):
            start = line.find('<', at)
            if start < 0: break
            if line.startswith('<!--', start):
                end = line.find('-->', start + 4)
                end = len(line) if end < 0 else end + 3
            elif re.match(r'</?[A-Za-z]', line[start:]):
                raw_tag = re.match(r'<(script|style|textarea|title)(?=[\s>])', line[start:], re.I)
                quote_char, end = None, start + 1
                while end < len(line):
                    ch = line[end]
                    if quote_char and ch == quote_char: quote_char = None
                    elif not quote_char and ch in "\"'": quote_char = ch
                    elif not quote_char and ch == '>': end += 1; break
                    end += 1
                if raw_tag:
                    close = re.search(rf'</{raw_tag.group(1)}\s*>', line[end:], re.I)
                    end = end + close.end() if close else len(line)
            else:
                at = start + 1
                continue
            protected.append((start, end))
            at = end
        changes = []
        at = 0
        while at < len(line):
            if line[at] == '\\' and at + 1 < len(line): at += 2; continue
            match = markdown_image_at(line, at)
            pdf_link = False
            if not match and line[at] == '[' and (not at or line[at - 1] != '!'):
                probe = markdown_image_at('!' + line[at:], 0)
                if probe:
                    match = tuple(at + position - 1 for position in probe)
                    pdf_link = True
            if not match: at += 1; continue
            begin, end, after = match
            if not any(start <= at < stop for start, stop in protected):
                raw = line[begin:end]
                decoded = re.sub(r'''\\([!"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])''', r'\1', raw)
                replacement = add(decoded) if not pdf_link or is_local_pdf(decoded) else decoded
                if replacement != decoded: changes.append((begin, end, replacement))
            at = after
        for start, end, replacement in reversed(changes): line = line[:start] + replacement + line[end:]
        return rewrite_html(line, images_only=True)

    def rewrite_html(source: str, images_only: bool = False) -> str:
        # Only actual media attributes and CSS are rewritten; script text and
        # comments stay literal.
        nonlocal inline_total
        offsets = [0] + [index + 1 for index, char in enumerate(source) if char == '\n']
        changes = []
        def inline_css(value: str) -> str:
            nonlocal inline_total
            raw = value.encode('utf-8')
            if len(raw) > MAX_ASSET:
                die('inline stylesheet is over 16 MiB')
            updated = bundle_css(base / '__markdownx_inline__.css', raw)
            inline_total += max(0, len(updated) - len(raw))
            if inline_total > MAX_TOTAL:
                die('inline stylesheet dependencies are over 64 MiB')
            return updated.decode('utf-8')
        class MediaParser(HTMLParser):
            in_style = False
            in_module = False
            in_script = False
            def handle_starttag(self, tag, attrs):
                if images_only and tag not in ('img', 'a'): return
                stylesheet = tag == 'link' and any(name == 'rel' and value and 'stylesheet' in value.lower().split() for name, value in attrs)
                module = tag == 'script' and any(name == 'type' and value and value.strip().lower() == 'module' for name, value in attrs)
                script_type = next((value.strip().lower() for name, value in attrs if name == 'type' and value), '')
                javascript = tag == 'script' and (module or script_type.split(';', 1)[0].strip() in ('', 'text/javascript', 'application/javascript', 'text/ecmascript', 'application/ecmascript', 'text/jscript', 'text/livescript'))
                media = tag in ('img', 'video', 'audio', 'source', 'track', 'model-viewer', 'script')
                raw = self.get_starttag_text()
                start = offsets[self.getpos()[0] - 1] + self.getpos()[1]
                if images_only and (len(source[:start]) - len(source[:start].rstrip('\\'))) % 2: return
                at = re.match(r'<\s*[^\s/>]+', raw).end()
                while at < len(raw):
                    match = re.match(r'''\s*([^\s=/>]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?''', raw[at:])
                    if not match:
                        break
                    name = match.group(1).lower()
                    value = next((group for group in (2, 3, 4) if match.group(group) is not None), None)
                    if value and name == 'style' and not images_only:
                        original = match.group(value)
                        decoded = html.unescape(original)
                        updated = inline_css(decoded)
                        if updated != decoded:
                            escaped = html.escape(updated, quote=True)
                            if value == 4: escaped = '"' + escaped + '"'
                            changes.append((start + at + match.start(value), start + at + match.end(value), escaped))
                    elif value and (media and name == 'src' or tag == 'video' and name == 'poster' or stylesheet and name == 'href' or tag == 'a' and name == 'href' and is_local_pdf(match.group(value))):
                        original = match.group(value)
                        updated = add(original, xml=True, stylesheet=stylesheet and name == 'href', module=module and name == 'src', javascript=javascript and name == 'src', media_time=tag in ('video', 'audio', 'source') and name == 'src')
                        if updated != html.unescape(original):
                            if updated.startswith('assets/'):
                                path, separator, fragment = updated.partition('#')
                                updated = './' + quote(path, safe='/') + (separator + fragment if separator else '')
                            escaped = html.escape(updated, quote=True)
                            if value == 4:
                                escaped = '"' + escaped + '"'
                            changes.append((start + at + match.start(value), start + at + match.end(value), escaped))
                    elif value and tag in ('img', 'source') and name == 'srcset':
                        original = match.group(value)
                        for begin, end in srcset_urls(original):
                            candidate = original[begin:end]
                            updated = add(candidate, xml=True)
                            if updated != html.unescape(candidate):
                                if updated.startswith('assets/'):
                                    path, separator, fragment = updated.partition('#')
                                    updated = './' + quote(path, safe='/') + (separator + fragment if separator else '')
                                escaped = html.escape(updated, quote=True)
                                if value == 4:
                                    tail = end
                                    while tail < len(original) and original[tail] == ',': tail += 1
                                    escaped = '"' + escaped + original[end:tail] + '"'
                                    end = tail
                                changes.append((start + at + match.start(value) + begin, start + at + match.start(value) + end, escaped))
                    at += match.end()
                if tag == 'style' and not images_only: self.in_style = True
                if tag == 'script' and javascript and not images_only:
                    self.in_script = True
                    self.in_module = module
            def handle_startendtag(self, tag, attrs):
                self.handle_starttag(tag, attrs)
                if tag == 'style': self.in_style = False
                if tag == 'script': self.in_module = False; self.in_script = False
            def handle_endtag(self, tag):
                if tag == 'style': self.in_style = False
                if tag == 'script': self.in_module = False; self.in_script = False
            def handle_data(self, data):
                if self.in_style:
                    start = offsets[self.getpos()[0] - 1] + self.getpos()[1]
                    updated = inline_css(data)
                    if updated != data: changes.append((start, start + len(data), updated))
                elif self.in_script and (self.in_module or may_reference_local_assets(data)):
                    nonlocal inline_total
                    start = offsets[self.getpos()[0] - 1] + self.getpos()[1]
                    raw = data.encode('utf-8')
                    updated = bundle_js(raw, base)
                    inline_total += max(0, len(updated) - len(raw))
                    if inline_total > MAX_TOTAL: die('inline module dependencies are over 64 MiB')
                    text = updated.decode('utf-8')
                    if text != data: changes.append((start, start + len(data), text))
        MediaParser(convert_charrefs=False).feed(source)
        for start, end, value in reversed(changes):
            source = source[:start] + value + source[end:]
        return source

    if not markdown:
        parts = re.split(r'''(<!\[CDATA\[.*?\]\]>|<!--.*?-->|<latex\b(?:"[^"]*"|'[^']*'|[^'">])*?>.*?</latex\s*>)''', content, flags=re.S | re.I)
        out = []
        in_html = False
        interactive = re.compile(r'''</?interactive\b(?:"[^"]*"|'[^']*'|[^'">])*?>''', re.I | re.S)
        for i, part in enumerate(parts):
            if i % 2:
                if in_html and part.startswith('<![CDATA['):
                    out.append('<![CDATA[' + rewrite_html(part[9:-3]) + ']]>')
                else:
                    out.append(part)
                continue
            out.append(rewrite(part))
            for tag in interactive.finditer(part):
                value = tag.group()
                if value.startswith('</'):
                    in_html = False
                else:
                    kind = re.search(r'''\btype\s*=\s*(["'])html\1''', value, re.I)
                    in_html = bool(kind) and not value.rstrip().endswith('/>')
        return ''.join(out)
    out, fence, in_comment = [], None, False
    lines = content.splitlines(keepends=True)
    code_lines = markdown_code_lines(lines)
    def rewrite_markdown_line(line: str) -> str:
        nonlocal in_comment
        # Hide complete math before scanning code, comments, images and links.
        spans = markdown_math_spans(line, in_comment)
        prefix = 'MDXPROTECTEDMATH'
        while prefix in line: prefix += 'X'
        protected = []
        for index, (start, end) in reversed(list(enumerate(spans))):
            token = f'{prefix}{index}END'
            protected.append((token, line[start:end]))
            line = line[:start] + token + line[end:]
        parts = re.split(r'(`+[^`\n]*`+)', line)
        rendered = []
        for part_index, part in enumerate(parts):
            if part_index % 2 and not in_comment: rendered.append(part); continue
            at = 0
            while at < len(part):
                if in_comment:
                    end = part.find('-->', at)
                    if end < 0: rendered.append(part[at:]); break
                    rendered.append(part[at:end + 3]); at = end + 3; in_comment = False
                else:
                    start = part.find('<!--', at)
                    if start < 0: rendered.append(rewrite(part[at:])); break
                    rendered.append(rewrite(part[at:start])); at = start; in_comment = True
        result = ''.join(rendered)
        for token, original in protected: result = result.replace(token, original)
        return result

    def table_cells(line: str) -> list[tuple[int, int]]:
        leading = len(line) - len(line.lstrip())
        start = leading + 1 if line[leading:leading + 1] == '|' else 0
        cells, at = [], start
        while at < len(line):
            if line[at:at + 2] == r'\|': at += 2; continue
            if line[at] == '|': cells.append((start, at)); start = at + 1
            at += 1
        if line[start:].strip(): cells.append((start, len(line)))
        return cells

    def rewrite_table_piece(raw: str) -> str:
        value = raw.strip().replace(r'\|', '|')
        encoded = re.fullmatch(r'<markdownx-interactive data-base64="([A-Za-z0-9+/=]+)"></markdownx-interactive>', value)
        replacement = None
        if encoded:
            try:
                payload = json.loads(base64.b64decode(encoded.group(1), validate=True).decode('utf-8'))
                if isinstance(payload, dict) and payload.get('kind') == 'html' and isinstance(payload.get('source'), str) and payload['source']:
                    updated = rewrite_html(payload['source'])
                    if updated != payload['source']:
                        payload['source'] = updated
                        data = base64.b64encode(json.dumps(payload, ensure_ascii=False, separators=(',', ':')).encode('utf-8')).decode('ascii')
                        replacement = f'<markdownx-interactive data-base64="{data}"></markdownx-interactive>'
            except (ValueError, TypeError, UnicodeDecodeError):
                pass
        else:
            tag = re.match(r'<(video|audio|canvas|model-viewer|div)(?=[\s/>])', value, re.I)
            if tag and (media_div_end([value], 0) == 1 if tag.group(1).lower() == 'div' else f'</{tag.group(1).lower()}>' in value.lower()):
                replacement = rewrite_html(value).replace('|', r'\|')
        if replacement is not None:
            left = len(raw) - len(raw.lstrip())
            right = len(raw.rstrip())
            return raw[:left] + replacement + raw[right:]
        return rewrite_markdown_line(raw)

    def rewrite_table_line(line: str) -> str:
        rendered, at = [], 0
        for start, end in table_cells(line):
            rendered.append(line[at:start])
            raw = line[start:end]
            pieces = raw.split('<br>') if '<markdownx-interactive data-base64="' in raw else [raw]
            rendered.append('<br>'.join(rewrite_table_piece(piece) for piece in pieces))
            at = end
        rendered.append(line[at:])
        return ''.join(rendered)

    def is_table_header(index: int) -> bool:
        if index + 1 >= len(lines) or '|' not in lines[index]: return False
        head, markers = table_cells(lines[index]), table_cells(lines[index + 1])
        return bool(head) and len(head) == len(markers) and all(re.fullmatch(r':?-+:?', lines[index + 1][a:b].strip()) for a, b in markers)

    def standalone_img_end(start: int) -> int | None:
        quote_char, size = None, 0
        for at in range(start, min(len(lines), start + 64)):
            size += len(lines[at])
            if size > 65536: return None
            for column, ch in enumerate(lines[at]):
                if quote_char and ch == quote_char: quote_char = None
                elif not quote_char and ch in "\"'": quote_char = ch
                elif not quote_char and ch == '>':
                    return at + 1 if not lines[at][column + 1:].strip() else None
        return None
    i = 0
    while i < len(lines):
        line = lines[i]
        if i in code_lines:
            out.append(line); i += 1; continue
        match = re.match(r'^\s{0,3}(`{3,}|~{3,})(.*)$', line) if not in_comment else None
        if not fence and not in_comment:
            end = display_math_end(lines, i)
            if end is not None:
                out.extend(lines[i:end]); i = end; continue
        if not fence and not in_comment and is_table_header(i):
            out.append(rewrite_table_line(lines[i])); out.append(lines[i + 1]); i += 2
            while i < len(lines) and lines[i].strip() and '|' in lines[i]:
                out.append(rewrite_table_line(lines[i])); i += 1
            continue
        media = re.match(r'^([ \t]*(?:(?:[-+*]|\d{1,9}[.)。])[ \t]+)?)<(video|audio|canvas|model-viewer|div)(?=[\s/>])', line, re.I) if not fence and not in_comment else None
        if media:
            prefix, tag = media.group(1), media.group(2).lower()
            list_indent = len(prefix.expandtabs(4)) if re.search(r'(?:[-+*]|\d{1,9}[.)。])[ \t]+$', prefix) else None
            def in_list(at: int) -> bool:
                return list_indent is None or at == i or not lines[at].strip() or len(re.match(r'^[ \t]*', lines[at]).group().expandtabs(4)) >= list_indent
            def close_after(start: int, name: str) -> int | None:
                for at in range(start, len(lines)):
                    if not in_list(at): return None
                    if name not in ('script', 'style') and at > start and re.match(r'^\s*(`{3,}|~{3,})', lines[at]): return None
                    if f'</{name}>' in lines[at].lower(): return at + 1
                return None
            def tag_after(start: int) -> int | None:
                quote = None
                for at in range(start, len(lines)):
                    if not in_list(at): return None
                    for ch in lines[at]:
                        if quote and ch == quote: quote = None
                        elif not quote and ch in "\"'": quote = ch
                        elif not quote and ch == '>': return at + 1
                return None
            end = media_div_end(lines, i, in_list) if tag == 'div' else close_after(i, tag)
            if end is not None:
                tail = lines[end - 1].lower().partition(f'</{tag}>')[2]
                if '<script' in tail and '</script>' not in tail:
                    end = close_after(end, 'script')
                elif '<style' in tail and '</style>' not in tail:
                    end = close_after(end, 'style')
                while end is not None:
                    next_line = end
                    while next_line < len(lines) and not lines[next_line].strip(): next_line += 1
                    if next_line == len(lines) or not in_list(next_line): break
                    if re.match(r'^\s*<script(?=[\s/>])', lines[next_line], re.I): end = close_after(next_line, 'script')
                    elif re.match(r'^\s*<style(?=[\s/>])', lines[next_line], re.I): end = close_after(next_line, 'style')
                    elif re.match(r'^\s*<link(?=[\s/>])', lines[next_line], re.I): end = tag_after(next_line)
                    else: break
                if end is not None:
                    out.append(rewrite_html(''.join(lines[i:end])))
                    i = end
                    continue
        raw_block = re.match(r'^\s*<(script|style|textarea|title)(?=[\s>])', line, re.I) if not fence and not in_comment else None
        if raw_block:
            close = re.compile(rf'</{raw_block.group(1)}\s*>', re.I)
            end = i
            while end < len(lines):
                end += 1
                if close.search(lines[end - 1]): break
            out.extend(lines[i:end])
            i = end
            continue
        if not fence and not in_comment and re.match(r'^\s*<img(?=[\s/>])', line, re.I):
            end = standalone_img_end(i)
            if end is not None:
                out.append(rewrite_html(''.join(lines[i:end]), images_only=True))
                i = end
                continue
        if not fence and match and match.group(2).strip().lower() == 'mdxb-html':
            marker = match.group(1)
            j = i + 1
            while j < len(lines):
                close = re.match(r'^\s{0,3}(`{3,}|~{3,})(.*)$', lines[j])
                if close and close.group(1)[0] == marker[0] and len(close.group(1)) >= len(marker) and not close.group(2).strip():
                    break
                j += 1
            raw = ''.join(lines[i + 1:j])
            try:
                payload = json.loads(raw)
                if isinstance(payload, dict) and isinstance(payload.get('source'), str):
                    updated = rewrite_html(payload['source'])
                    if updated != payload['source']:
                        payload['source'] = updated
                        raw = json.dumps(payload, ensure_ascii=False) + ('\r\n' if line.endswith('\r\n') else '\n')
            except (ValueError, TypeError):
                pass
            out.extend((line, raw))
            if j < len(lines): out.append(lines[j])
            i = j + 1
            continue
        if fence:
            out.append(line)
            if match and match.group(1)[0] == fence[0] and len(match.group(1)) >= len(fence) and not match.group(2).strip():
                fence = None
        elif match:
            fence = match.group(1)
            out.append(line)
        elif not in_comment and reference_definition(line):
            out.append(line)
        else:
            out.append(rewrite_markdown_line(line))
        i += 1
    return ''.join(out)


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
        ".gif": "image/gif", ".webp": "image/webp", ".svg": "image/svg+xml", ".woff": "font/woff", ".woff2": "font/woff2", ".ttf": "font/ttf", ".otf": "font/otf", ".eot": "application/vnd.ms-fontobject",
        ".pdf": "application/pdf", ".mp4": "video/mp4", ".m4v": "video/mp4", ".mov": "video/quicktime", ".webm": "video/webm", ".ogv": "video/ogg", ".mp3": "audio/mpeg", ".wav": "audio/wav", ".ogg": "audio/ogg", ".opus": "audio/ogg", ".m4a": "audio/mp4", ".aac": "audio/aac", ".flac": "audio/flac", ".vtt": "text/vtt", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".obj": "model/obj", ".stl": "model/stl", ".gltf": "model/gltf+json", ".glb": "model/gltf-binary",
    }.get(path.suffix.lower(), "application/octet-stream")


def chapter_from_markdown(path: Path, root: Path, assets: dict[str, dict]) -> dict:
    text = path.read_text(encoding="utf-8")
    title = path.stem
    lines = text.splitlines()
    first = next((i for i, line in enumerate(lines) if line.strip()), None)
    if first is not None and lines[first].startswith("# "):
        title = lines[first][2:].strip() or title
        del lines[first]
        if first < len(lines) and not lines[first].strip():
            del lines[first]
    # Remove surrounding blank lines without stripping meaningful indentation
    # from the first or last code block.
    start = next((i for i, line in enumerate(lines) if line.strip()), len(lines))
    end = next((i + 1 for i in range(len(lines) - 1, start - 1, -1) if lines[i].strip()), start)
    body = "\n".join(lines[start:end]) + "\n"
    return {
        "id": slug(path.stem),
        "title": {"zh": title, "en": title},
        "format": "markdown",
        "content": embed_assets(body, root, assets, path.parent, markdown=True),
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
        "content": embed_assets(text, root, assets, path.parent, markdown=False),
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
        chapter["source_path"] = kept.get("source_path", kept.get("file", relative)) if kept else relative
        english_name = kept.get("content_en_file") if kept else None
        english = (root / english_name).resolve() if english_name else path.with_name(path.stem + ".en.md")
        try:
            english.relative_to(root)
        except ValueError:
            die(f"translation is outside the book folder: {english_name}")
        if english.is_file():
            fmt_en = (kept.get("format_en") if kept else None) or ("docxxml" if english.suffix.lower() in (".xml", ".docxxml") else "markdown")
            chapter["content_en"] = embed_assets(english.read_text(encoding="utf-8"), root, assets, english.parent, markdown=fmt_en in ("markdown", "md"))
            chapter["source_en_path"] = kept.get("source_en_path", english.relative_to(root).as_posix()) if kept else english.relative_to(root).as_posix()
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
