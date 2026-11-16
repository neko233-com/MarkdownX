"""Nested Markdown list layout, image packaging and code safety in the reader."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required for browser-reader checks')
class WebListTests(unittest.TestCase):
    def test_list_media_assets_are_packaged_and_remain_interactive(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-web-list-media-') as tmp:
            root = Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / 'clip.mp4').write_bytes(b'local video')
            (source / 'game.js').write_text("document.body.dataset.ready='yes';", encoding='utf-8')
            text = ('# Story\n\n3。 <video controls>\n   <source src="./clip.mp4">\n   </video>\n\n'
                    '4。 <canvas></canvas>\n   <script src="game.js"></script>\n\n'
                    '       <video src="missing.mp4"></video>\n')
            chapter = source / 'story.md'
            chapter.write_text(text, encoding='utf-8')
            book = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(source), '-o', str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            packed = json.loads(book.read_text(encoding='utf-8'))
            self.assertEqual(len(packed['assets']), 2)
            self.assertEqual(chapter.read_text(encoding='utf-8'), text)
            (root / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('markdown.js', 'docxml.js', 'katex.mjs', 'mdxb.js'):
                shutil.copyfile(ROOT / 'web/js' / name, root / name)
            (root / 'check.mjs').write_text(r"""
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {normalizeBook} from './mdxb.js';
const book = normalizeBook(JSON.parse(readFileSync(process.argv[2], 'utf8')));
const list = book.chapters[0].blocks[0];
assert.equal(list.start, 3);
assert.equal(list.items.length, 2);
const video = list.items[0].blocks[0];
const game = list.items[1].blocks[0];
assert.equal(video.type, 'interactive');
assert.match(video.source, /<video/);
assert.match(video.source, /assets\/[^" ]+\.mp4/);
assert.equal(game.type, 'interactive');
assert.match(game.source, /<canvas/);
assert.match(game.source, /assets\/[^" ]+\.js/);
assert.equal(list.items[1].blocks[1].type, 'code');
assert.match(list.items[1].blocks[1].code, /missing\.mp4/);
""", encoding='utf-8')
            check = subprocess.run(['node', str(root / 'check.mjs'), str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(check.returncode, 0, check.stdout + check.stderr)
            (source / 'clip.mp4').unlink()
            missing = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(source), '-o', str(root / 'missing.mdxb')], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(missing.returncode, 0)
            self.assertFalse((root / 'missing.mdxb').exists())

    def test_nested_list_resources_and_notes_survive_pack_and_read(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-web-lists-') as tmp:
            root = Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / 'shot.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"/>', encoding='utf-8')
            text = ('# Paper\n\n3。 Parent[^n]\n   - Child\n\n     ![frame](./shot.svg)\n\n'
                    '         ![literal](missing.png)\n\n4。 Next\n\n[^n]: Note.\n')
            chapter = source / 'paper.md'
            chapter.write_text(text, encoding='utf-8')
            book = root / 'book.mdxb'
            packed = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(source), '-o', str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(packed.returncode, 0, packed.stdout + packed.stderr)
            self.assertEqual(chapter.read_text(encoding='utf-8'), text)
            self.assertEqual(len(json.loads(book.read_text(encoding='utf-8'))['assets']), 1)
            (root / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('markdown.js', 'docxml.js', 'katex.mjs', 'mdxb.js'):
                shutil.copyfile(ROOT / 'web/js' / name, root / name)
            (root / 'check.mjs').write_text(r"""
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {normalizeBook} from './mdxb.js';
import {parseMarkdown} from './markdown.js';
const book = normalizeBook(JSON.parse(readFileSync(process.argv[2], 'utf8')));
const blocks = book.chapters[0].blocks;
const list = blocks.find(b => b.type === 'list');
assert.equal(list.ordered, true);
assert.equal(list.start, 3);
assert.equal(list.items.length, 2);
assert.match(list.items[0].html, /footnote-ref/);
assert.equal(list.items[1].html, 'Next');
const child = list.items[0].blocks[0];
assert.equal(child.type, 'list');
assert.equal(child.ordered, false);
assert.equal(child.items[0].html, 'Child');
const [image, code] = child.items[0].blocks;
assert.equal(image.type, 'image');
assert.ok(image.src.startsWith('./assets/'));
assert.equal(code.type, 'code');
assert.equal(code.code, '![literal](missing.png)');
assert.equal(blocks.find(b => b.type === 'footnotes').notes.length, 1);
const other = parseMarkdown('- [x] Done\n  2) Nested\n  3) Another\n- [ ] Todo\n\nOutside\n');
assert.equal(other.blocks[0].items[0].done, true);
assert.equal(other.blocks[0].items[1].done, false);
assert.equal(other.blocks[0].items[0].blocks[0].start, 2);
assert.equal(other.blocks[0].items[0].blocks[0].items.length, 2);
assert.equal(other.blocks[1].html, 'Outside');
const markerCode = parseMarkdown('-     ![literal](missing.png)\n      second\n');
assert.equal(markerCode.blocks[0].items[0].blocks[0].type, 'code');
assert.equal(markerCode.blocks[0].items[0].blocks[0].code, '![literal](missing.png)\nsecond');
const separate = parseMarkdown('- Bullet\n1. Ordered\n');
assert.deepEqual(separate.blocks.map(b => b.ordered), [false, true]);
""", encoding='utf-8')
            result = subprocess.run(['node', str(root / 'check.mjs'), str(book)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
