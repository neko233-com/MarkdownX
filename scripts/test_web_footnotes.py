"""Footnote rendering for Markdown chapters and their packed local assets."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required for browser-reader checks')
class WebFootnoteTests(unittest.TestCase):
    def test_packed_footnotes_keep_assets_literals_and_navigation_targets(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-web-footnotes-') as tmp:
            root = Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / 'image.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"/>', encoding='utf-8')
            (source / 'paper.md').write_text(
                '# Paper\n\nFirst[^a], again[^a], missing[^none].\n\n'
                '> quote[^a]\n\n| A | B |\n|---|---|\n| x | [^b] |\n\n'
                '[^a]: parent[^b] with **bold**.\n\n    More ![img](./image.svg).\n\n'
                '[^b]: child.\n\n[^unused]: keep this\n\n'
                '`[^a]` and \\[^a]\n\n```md\n[^a]\n```\n', encoding='utf-8')
            book = root / 'book.mdxb'
            proc = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(source), '-o', str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            packed = json.loads(book.read_text(encoding='utf-8'))
            self.assertTrue(packed['assets'])
            self.assertIn('[^a]:', packed['chapters'][0]['content'])
            (root / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('markdown.js', 'docxml.js', 'katex.mjs', 'mdxb.js'):
                shutil.copyfile(ROOT / 'web/js' / name, root / name)
            (root / 'check.mjs').write_text(r"""
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {normalizeBook, headingSlug} from './mdxb.js';
import {parseMarkdown} from './markdown.js';
const book = normalizeBook(JSON.parse(readFileSync(process.argv[2], 'utf8')));
const blocks = book.chapters[0].blocks;
const notes = blocks.find(b => b.type === 'footnotes').notes;
assert.equal(notes.length, 2);
assert.equal(notes[0].backlinks.length, 3);
assert.equal(notes[1].backlinks.length, 2);
assert.match(blocks.find(b => b.type === 'table').rows[1][1].html, new RegExp(`href="#${notes[1].id}"`));
assert.match(notes[0].blocks[0].html, /<strong>bold<\/strong>/);
assert.match(notes[0].blocks[1].html, /src="data:image\/svg\+xml;base64,/);
assert.ok(blocks.some(b => b.html?.includes('[^unused]: keep this')));
assert.ok(blocks.some(b => b.html?.includes('[^none]')));
assert.ok(blocks.some(b => b.html?.includes('<code>[^a]</code>')));
assert.ok(blocks.some(b => b.type === 'code' && b.code === '[^a]'));
const duplicates = parseMarkdown('Text[^x].\n\n[^x]: one\n\n[^x]: two\n');
assert.ok(!duplicates.blocks.some(b => b.type === 'footnotes'));
assert.ok(duplicates.blocks.some(b => b.html?.includes('[^x]: one')));
const ignored = parseMarkdown('`[^x]` and \\[^x]\n\n[^x]: unused\n');
assert.ok(!ignored.blocks.some(b => b.type === 'footnotes'));
const masked = parseMarkdown('Real[^x].\n\n<!-- begin\n\n[^x] hidden\n-->\n\nMath $[^x]$ and <code>[^x]</code>.\n\n[^x]: note\n');
assert.equal(masked.blocks.at(-1).notes[0].backlinks.length, 1);
assert.ok(masked.blocks.some(b => b.html?.includes('hidden')));
const codeExamples = parseMarkdown('Real[^x].\n\n  ```md\n[^x] inside fence\n  ```\n\n    [^x] inside indented code\n\n[^x]: note\n');
assert.equal(codeExamples.blocks.at(-1).notes[0].backlinks.length, 1);
assert.deepEqual(codeExamples.blocks.filter(b => b.type === 'code').map(b => b.code), ['[^x] inside fence', '[^x] inside indented code']);
const hostile = parseMarkdown('Text[^a].\n\n[^a]: <img src=x onerror=alert(1)>\n');
assert.doesNotMatch(JSON.stringify(hostile.blocks.at(-1).notes[0].blocks), /onerror=/);
const headings = parseMarkdown('# Paper[^a]\n\n## Section[^a]\n\n## Section[^a]\n\n[^a]: note\n');
const h = headings.blocks.filter(b => b.type === 'h');
assert.deepEqual(h.map(b => b.anchorHtml), ['Paper[^a]', 'Section[^a]', 'Section[^a]']);
assert.ok(h.every(b => b.html.includes('class="footnote-ref"')));
const media = parseMarkdown(String.raw`## 图解 ![镜头](./shot.svg)

## 公式 $\frac{1}{2}$`, '', () => 'https://example.test/shot.svg').blocks.filter(b => b.type === 'h');
assert.equal(media[0].anchorHtml, '图解 镜头');
assert.equal(media[1].anchorHtml, String.raw`公式 \frac{1}{2}`);
assert.match(media[0].html, /<img /);
assert.match(media[1].html, /class="math-inline"/);
assert.equal(headingSlug('ΟΣ'), 'οσ');
assert.equal(headingSlug('\u0085 图解 镜头 \u0085'), '图解-镜头');
""", encoding='utf-8')
            proc = subprocess.run(['node', str(root / 'check.mjs'), str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)


if __name__ == '__main__':
    unittest.main()
