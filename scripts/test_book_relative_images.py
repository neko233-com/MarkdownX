"""Exercise chapter-relative image packaging and the browser reader together."""
import base64
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
TOOL = ROOT / 'tools' / 'agent' / 'mdxb.py'


class RelativeImageTests(unittest.TestCase):
    def test_math_does_not_resolve_literal_resources_in_chapters_or_translations(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-book-math-assets-') as tmp:
            root = Path(tmp)
            (root / 'real.svg').write_bytes(b'real image')
            (root / '$take$.svg').write_bytes(b'dollar image')
            chapter = root / 'story.md'
            translation = root / 'story.en.md'
            source = ('# Story\n\n' + r'$\text{![fake](missing.png)}$' + '\n\n'
                      + r'$\text{![ref][shot]}$' + '\n\n'
                      + r'$\text{<img src="missing.png">}$' + '\n\n'
                      + r'$\text{[pdf](missing.pdf#page=2)}$' + '\n\n'
                      + r'$\text{<!-- unfinished}$' + '\n\n'
                      + '$$\n' + r'\text{![block][shot]}' + '\n$$\n\n'
                      + '> $$\n> ' + r'\text{![quote][shot]}' + '\n> $$\n\n'
                      + '![real](./real.svg) ![dollar](./$take$.svg)\n\n[shot]: missing.png\n')
            chapter.write_text(source, encoding='utf-8')
            translation.write_text(source.replace('# Story', '# English'), encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            for body in (book['chapters'][0]['content'], book['chapters'][0]['content_en']):
                for literal in (r'$\text{![fake](missing.png)}$', r'$\text{![ref][shot]}$',
                                r'$\text{<img src="missing.png">}$', r'$\text{[pdf](missing.pdf#page=2)}$',
                                r'$\text{<!-- unfinished}$', r'\text{![block][shot]}', r'\text{![quote][shot]}'):
                    self.assertIn(literal, body)
                self.assertEqual(body.count('./assets/'), 2, body)
            data = [base64.b64decode(asset['data']) for asset in book['assets'].values()]
            self.assertEqual(sorted(data), [b'dollar image', b'real image'])
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            sys.path.insert(0, str(TOOL.parent))
            try:
                import mdxb
                xml = r'<p><latex display="block">\text{![fake](missing.png)} &amp; \text{&lt;img src="missing.png"&gt;}</latex></p>'
                self.assertEqual(mdxb.embed_assets(xml, root, {}, root, markdown=False), xml)
            finally:
                sys.path.pop(0)

    def test_pdf_links_in_markdown_and_docxml_keep_page_and_local_precedence(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-pdf-links-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            local = b'%PDF-1.4 local'
            (root / 'paper.pdf').write_bytes(b'%PDF-1.4 root')
            (chapter / 'paper.pdf').write_bytes(local)
            source = ('# Story\n\n[read](./paper.pdf#page=2) [reference][paper]\n\n'
                      '[paper]: paper.pdf#page=3 "Title"\n\n'
                      '`[example](./paper.pdf#page=4)`\n\n'
                      '```markdown\n[example](./paper.pdf#page=5)\n```\n')
            file = chapter / 'story.md'
            file.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            content = book['chapters'][0]['content']
            self.assertRegex(content, r'\./assets/[a-f0-9]+\.pdf#page=2')
            self.assertRegex(content, r'\./assets/[a-f0-9]+\.pdf#page=3')
            self.assertIn('`[example](./paper.pdf#page=4)`', content)
            self.assertIn('[example](./paper.pdf#page=5)', content)
            self.assertIn(local, [base64.b64decode(asset['data']) for asset in book['assets'].values()])
            self.assertEqual(file.read_text(encoding='utf-8'), source)
            sys.path.insert(0, str(TOOL.parent))
            try:
                import mdxb
                assets = {}
                xml = mdxb.embed_assets('<p><a href="./paper.pdf#page=2">read</a></p>', root, assets, chapter, markdown=False)
                self.assertRegex(xml, r'assets/[a-f0-9]+\.pdf#page=2')
                self.assertIn(local, [base64.b64decode(asset['data']) for asset in assets.values()])
                self.assertFalse(mdxb.is_local_pdf('paper.pdf#page=0'))
                self.assertFalse(mdxb.is_local_pdf('https://example.com/paper.pdf#page=2'))
            finally:
                sys.path.pop(0)
            outside = root.parent / (root.name + '-outside.pdf')
            try:
                outside.write_bytes(local)
                file.write_text('# Story\n\n[unsafe](../../' + outside.name + '#page=2)\n', encoding='utf-8')
                failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(root / 'unsafe.mdxb')], capture_output=True, text=True, encoding='utf-8')
                self.assertNotEqual(failed.returncode, 0)
                self.assertFalse((root / 'unsafe.mdxb').exists())
            finally:
                outside.unlink(missing_ok=True)

    def test_svg_fragments_pack_in_markdown_and_html_without_escaping_root(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-svg-view-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            svg = b'<svg xmlns="http://www.w3.org/2000/svg"><view id="right" viewBox="50 0 50 50"/></svg>'
            (chapter / 'frame#take.svg').write_bytes(svg)
            source = ("# Story\n\n![view](./frame%23take.svg?v=1#right)\n\n"
                      "| Shot | Frame |\n| --- | --- |\n| 1 | ![view](<frame%23take.svg#svgView(viewBox(50,0,50,50))>) |\n\n"
                      "<picture><source srcset='./frame%23take.svg#right 1x'><img src='./frame%23take.svg#right'></picture>\n")
            primary = chapter / 'story.md'
            primary.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            content = book['chapters'][0]['content']
            self.assertEqual(content.count('#right'), 3)
            self.assertIn('#svgView(viewBox(50,0,50,50))', content)
            self.assertNotIn('%23right', content)
            self.assertIn(svg, [base64.b64decode(asset['data']) for asset in book['assets'].values()])
            self.assertEqual(primary.read_text(encoding='utf-8'), source)
            (root / 'private.svg').write_bytes(svg)
            primary.write_text('# Story\n\n![private](../private.svg#right)\n', encoding='utf-8')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(chapter), '-o', str(root / 'unsafe.mdxb')], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertFalse((root / 'unsafe.mdxb').exists())

    @unittest.skipUnless(shutil.which('node'), 'Node.js is required for URL normalization checks')
    def test_module_identity_matches_browser_url_serialization(self):
        sys.path.insert(0, str(TOOL.parent))
        previous_bytecode = sys.dont_write_bytecode
        sys.dont_write_bytecode = True
        try:
            import mdxb
        finally:
            sys.dont_write_bytecode = previous_bytecode
            sys.path.pop(0)
        values = ['?v=中', '?v=%E4%B8%AD', '?q=a b#图', '?q=a%20b#%E5%9B%BE',
                  "?q='quote'", '?q=%3f', '?q=%3F', '?x=a\tb', '?', '#', '?q=trailing ',
                  '?q=!$%&()*+,-./:;=?@[]\\^_`{|}~', '?q="<>\' #`~[]', '#?q=中', '?q=a\x01b']
        values += ['?q=a' + chr(code) + 'b' for code in range(128)]
        values += ['#a' + chr(code) + 'b' for code in range(128)]
        values += ['?q=a' + chr(code) for code in range(33)]
        values += ['#a' + chr(code) for code in range(33)]
        values += ['?q=é😀\u00a0', '#图😀\u200b']
        script = '''const fs=require('fs'), values=JSON.parse(fs.readFileSync(0,'utf8')); const base='https://markdownx.invalid/module.js'; console.log(JSON.stringify(values.map(s=>{const raw=new URL(base+s).href.slice(base.length);return raw?'#'+[...new TextEncoder().encode(raw)].map(b=>'%'+b.toString(16).toUpperCase().padStart(2,'0')).join(''):''})));'''
        result = subprocess.run(['node', '-e', script], input=json.dumps(values, ensure_ascii=False), capture_output=True, text=True, encoding='utf-8')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([mdxb.module_identity(value) for value in values], json.loads(result.stdout))

    def test_script_queries_keep_module_identity_in_chapters_and_translations(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-script-query-') as tmp:
            root = Path(tmp)
            part = root / 'chapter'
            part.mkdir()
            (part / 'game.mjs').write_text('globalThis.runs=(globalThis.runs||0)+1;', encoding='utf-8')
            source = ("# Story\n\n<canvas></canvas>\n"
                      "<script type='module' src='./game.mjs?v=1'></script>\n"
                      "<script type='module' src='./game.mjs?v=1'></script>\n"
                      "<script type='module' src='game.mjs?v=2'></script>\n"
                      "<script type='module' src='game.mjs?v=中'></script>\n"
                      "<script type='module' src='game.mjs?v=%E4%B8%AD'></script>\n"
                      "<script type='module' src='game.mjs#%3F%76%3D%E4%B8%AD'></script>\n")
            chapter = part / 'story.md'
            translation = part / 'story.en.md'
            chapter.write_text(source, encoding='utf-8')
            translation.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            for text in (book['chapters'][0]['content'], book['chapters'][0]['content_en']):
                self.assertEqual(text.count('#%3F%76%3D%31'), 2)
                self.assertEqual(text.count('#%3F%76%3D%32'), 1)
                self.assertEqual(text.count('#%3F%76%3D%25%45%34%25%42%38%25%41%44'), 3)
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            self.assertEqual(translation.read_text(encoding='utf-8'), source)
            (root / 'outside.mjs').write_text('globalThis.bad=true;', encoding='utf-8')
            chapter.write_text("# Story\n\n<canvas></canvas>\n<script type='module' src='../outside.mjs?v=1'></script>\n", encoding='utf-8')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(part), '-o', str(root / 'unsafe.mdxb')], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertFalse((root / 'unsafe.mdxb').exists())

    def test_query_parameters_embed_chapter_media_and_keep_originals(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-query-book-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            (root / 'frame.svg').write_bytes(b'wrong-root')
            (chapter / 'frame.svg').write_bytes(b'chapter-image')
            (chapter / 'clip.mp4').write_bytes(b'chapter-video')
            body = ("![cover](./frame.svg?v=1)\n\n| Shot | Image |\n| --- | --- |\n"
                    "| 1 | ![frame](frame.svg?cache=2) |\n\n"
                    "<video src='./clip.mp4?v=3#t=2,5' poster='frame.svg?v=4'></video>\n")
            primary, english = chapter / 'story.md', chapter / 'story.en.md'
            primary.write_text('# Story\n\n' + body, encoding='utf-8')
            english.write_text('# English\n\n' + body, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            assets = [base64.b64decode(asset['data']) for asset in book['assets'].values()]
            self.assertIn(b'chapter-image', assets)
            self.assertIn(b'chapter-video', assets)
            self.assertNotIn(b'wrong-root', assets)
            for text in (book['chapters'][0]['content'], book['chapters'][0]['content_en']):
                self.assertIn('./assets/', text)
                self.assertNotIn('frame.svg?v=', text)
                self.assertNotIn('clip.mp4?v=', text)
                self.assertIn('#t=2,5', text)
            self.assertEqual(primary.read_text(encoding='utf-8'), '# Story\n\n' + body)
            self.assertEqual(english.read_text(encoding='utf-8'), '# English\n\n' + body)
            (root / 'private.svg').write_bytes(b'outside')
            primary.write_text('# Story\n\n![private](../private.svg?v=1)\n', encoding='utf-8')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(chapter), '-o', str(root / 'unsafe.mdxb')], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn('outside', failed.stderr.lower())
            self.assertFalse((root / 'unsafe.mdxb').exists())

    def test_widget_srcset_packages_each_candidate_from_chapter(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-srcset-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            (root / 'small.svg').write_bytes(b'wrong-root')
            (chapter / 'small.svg').write_bytes(b'chapter-small')
            (chapter / 'big.svg').write_bytes(b'chapter-big')
            source = ("# Story\n\n<div><canvas></canvas><picture>"
                      "<source srcset='./small.svg 1x, big.svg 2x'>"
                      "<img srcset=small.svg,></picture>"
                      "<img srcset='data:image/png;base64,AAAA 1x, ./big.svg 2x'></div>\n")
            document = chapter / 'story.md'
            document.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            assets = [base64.b64decode(asset['data']) for asset in book['assets'].values()]
            self.assertIn(b'chapter-small', assets)
            self.assertIn(b'chapter-big', assets)
            self.assertNotIn(b'wrong-root', assets)
            content = book['chapters'][0]['content']
            small = next(key for key, asset in book['assets'].items() if base64.b64decode(asset['data']) == b'chapter-small')
            big = next(key for key, asset in book['assets'].items() if base64.b64decode(asset['data']) == b'chapter-big')
            self.assertIn(f"./{small} 1x, ./{big} 2x", content)
            self.assertIn(f'srcset="./{small},"', content)
            self.assertIn(f'data:image/png;base64,AAAA 1x, ./{big} 2x', content)
            self.assertEqual(document.read_text(encoding='utf-8'), source)

    def test_table_media_embeds_relative_assets_in_both_languages(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-book-table-media-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (part / 'clip.mp4').write_bytes(b'local-video')
            (root / 'clip.mp4').write_bytes(b'wrong-video')
            (part / 'sound.wav').write_bytes(b'local-audio')
            (root / 'sound.wav').write_bytes(b'wrong-audio')
            widget = {'kind': 'html', 'title': 'game', 'height': 240,
                      'source': "<video src='clip.mp4'></video>\n<script>let title='中文 | x';</script>"}
            encoded = base64.b64encode(json.dumps(widget, ensure_ascii=False).encode('utf-8')).decode('ascii')
            wrapper = f'<markdownx-interactive data-base64="{encoded}"></markdownx-interactive>'
            body = "| Shot | Media |\n| --- | --- |\n| 1 | <video src='./clip.mp4'></video> |\n" + f'| 2 | before<br>{wrapper} |\n' + "| 3 | <audio controls><source src='sound.wav'></audio> |\n"
            chapter, english = part / 'story.md', part / 'story.en.md'
            chapter.write_text('# Story\n\n' + body, encoding='utf-8')
            english.write_text('# English\n\n' + body, encoding='utf-8')
            out = root / 'book.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            self.assertTrue(any(base64.b64decode(asset['data']) == b'local-video' for asset in book['assets'].values()))
            self.assertTrue(any(base64.b64decode(asset['data']) == b'local-audio' and asset['mime'] == 'audio/wav' for asset in book['assets'].values()))
            for text in (book['chapters'][0]['content'], book['chapters'][0]['content_en']):
                self.assertIn("<video src='./assets/", text)
                self.assertIn("<audio controls><source src='./assets/", text)
                match = re.search(r'data-base64="([A-Za-z0-9+/=]+)"', text)
                self.assertIsNotNone(match)
                source = json.loads(base64.b64decode(match.group(1)))['source']
                self.assertIn('./assets/', source)
                self.assertIn('中文 | x', source)
            (part / 'clip.mp4').unlink()
            out.write_bytes(b'keep existing output')
            failed = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep existing output')

    def test_wrapped_canvas_and_video_assets_use_the_chapter_directory(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-div-widget-') as tmp:
            root = Path(tmp)
            part = root / 'chapter'
            part.mkdir()
            (part / 'game.js').write_text("document.querySelector('canvas').dataset.ready='yes';", encoding='utf-8')
            (part / 'clip.mp4').write_bytes(b'chapter-video')
            (root / 'clip.mp4').write_bytes(b'wrong-video')
            source = ("# Story\n\n<div class='game'>\n<!-- </div><video src='missing.mp4'></video> -->\n"
                      "<div><canvas></canvas></div>\n<script>const sample='</div>';</script>\n</div>\n"
                      "<script src='./game.js'></script>\n\n| Shot | Video |\n| --- | --- |\n"
                      "| 1 | <div><video src='clip.mp4'></video></div> |\n\n"
                      "```html\n<div><video src='missing.mp4'></video></div>\n```\n")
            chapter = part / 'story.md'
            chapter.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            self.assertTrue(any(base64.b64decode(asset['data']) == b'chapter-video' for asset in book['assets'].values()))
            self.assertTrue(any(b'dataset.ready' in base64.b64decode(asset['data']) for asset in book['assets'].values()))
            content = book['chapters'][0]['content']
            self.assertIn("<script src='./assets/", content)
            self.assertIn("<video src='./assets/", content)
            self.assertIn("<video src='missing.mp4'>", content)
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)

    def test_chapter_source_paths_survive_cli_pack_unpack_and_repack(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-book-paths-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (part / '01-first.md').write_text('# First\n\n[Next](./02-second.md)\n', encoding='utf-8')
            (part / '02-second.md').write_text('# Second\n\n[Back](01-first.md)\n', encoding='utf-8')
            first = root / 'first.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(first)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(first.read_text(encoding='utf-8'))
            self.assertEqual([ch['source_path'] for ch in book['chapters']], ['part/01-first.md', 'part/02-second.md'])
            extracted = root / 'unpacked'
            result = subprocess.run([sys.executable, str(TOOL), 'unpack', str(first), str(extracted)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            second = root / 'second.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(extracted), '-o', str(second)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            repacked = json.loads(second.read_text(encoding='utf-8'))
            self.assertEqual([ch['source_path'] for ch in repacked['chapters']], ['part/01-first.md', 'part/02-second.md'])
            self.assertIn('[Next](./02-second.md)', repacked['chapters'][0]['content'])

    def test_reference_images_package_from_each_chapter_directory(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-image-reference-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (part / 'shot.svg').write_bytes(b'chapter-image')
            (root / 'shot.svg').write_bytes(b'wrong-image')
            chapter = part / 'story.md'
            source = ('# Story\n\n![cover][ SHOT ]\n\n| No | Frame |\n|---|---|\n'
                      '| 1 | before ![detail][] after |\n| 2 | ![shortcut] |\n\n'
                      '[shot]: ./shot.svg "cover"\n[DETAIL]: shot.svg "left|right"\n[shortcut]: ./shot.svg\n'
                      '[shot]: missing.png\n[spare]: ./shot.svg "![literal](missing.png)"\n\n`![code][shot]`\n\n'
                      'text <!--\n[fake]: missing.png\n![hidden][shot]\n-->\n\n'
                      '```md\n![code][shot]\n[example]: missing.png\n```\n')
            chapter.write_text(source, encoding='utf-8')
            translation = part / 'story.en.md'
            english = '# English\n\n![English][frame]\n\n[frame]: shot.svg\n'
            translation.write_text(english, encoding='utf-8')
            out = root / 'book.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            body = book['chapters'][0]['content']
            self.assertEqual(body.count('./assets/'), 3)
            self.assertIn('`![code][shot]`', body)
            self.assertIn('![hidden][shot]', body)
            self.assertIn('[fake]: missing.png', body)
            self.assertIn('[shot]: ./shot.svg "cover"', body)
            self.assertIn('[spare]: ./shot.svg "![literal](missing.png)"', body)
            self.assertIn('![code][shot]', body)
            self.assertIn('./assets/', book['chapters'][0]['content_en'])
            self.assertIn('[frame]: shot.svg', book['chapters'][0]['content_en'])
            self.assertEqual(base64.b64decode(next(iter(book['assets'].values()))['data']), b'chapter-image')
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            self.assertEqual(translation.read_text(encoding='utf-8'), english)
            (part / 'shot.svg').unlink()
            out.write_bytes(b'keep existing output')
            failed = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep existing output')

    def test_balanced_and_escaped_image_paths_package_from_chapter(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-image-parens-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (part / 'shot(1).svg').write_bytes(b'local-frame')
            (part / 'frame two.svg').write_bytes(b'spaced-frame')
            chapter = part / 'story.md'
            source = ('# Story\n\n![frame [one]](./shot(1).svg "caption")\n\n'
                      '| Shot | Image |\n| --- | --- |\n| 1 | before ![frame](shot\\(1\\).svg) after |\n\n'
                      '![space](<./frame two.svg>)\n\n\\![literal](missing.png)\n\n'
                      '`![example](missing.png)`\n')
            chapter.write_text(source, encoding='utf-8')
            out = root / 'book.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            content = book['chapters'][0]['content']
            self.assertEqual(content.count('./assets/'), 3)
            self.assertIn('"caption"', content)
            self.assertIn('\\![literal](missing.png)', content)
            self.assertIn('`![example](missing.png)`', content)
            self.assertEqual({base64.b64decode(a['data']) for a in book['assets'].values()}, {b'local-frame', b'spaced-frame'})
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            (part / 'shot(1).svg').unlink()
            out.write_bytes(b'keep existing output')
            failed = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep existing output')

    def test_gltf_and_glb_embed_relative_buffers_without_modifying_originals(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-model-deps-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (part / 'mesh data.bin').write_bytes(b'geometry')
            model = {'asset': {'version': '2.0'}, 'buffers': [{'uri': './mesh%20data.bin', 'byteLength': 8}]}
            raw = json.dumps(model, separators=(',', ':')).encode()
            (part / 'shape.gltf').write_bytes(raw)
            chunk = raw + b' ' * (-len(raw) % 4)
            glb = b'glTF' + struct.pack('<II', 2, 20 + len(chunk)) + struct.pack('<II', len(chunk), 0x4e4f534a) + chunk
            (part / 'shape.glb').write_bytes(glb)
            out = root / 'book.mdxb'
            for name in ('shape.gltf', 'shape.glb'):
                (part / 'chapter.md').write_text(f"<model-viewer src='./{name}'></model-viewer>\n", encoding='utf-8')
                result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                book = json.loads(out.read_text(encoding='utf-8'))
                self.assertEqual(len(book['assets']), 2)
                data = {key: base64.b64decode(value['data']) for key, value in book['assets'].items()}
                payload = next(value for key, value in data.items() if key.endswith('.' + name.rsplit('.', 1)[-1]))
                if name.endswith('.glb'):
                    length = struct.unpack_from('<I', payload, 12)[0]
                    payload = payload[20:20 + length]
                dependency = json.loads(payload)['buffers'][0]['uri']
                self.assertTrue(dependency.startswith('./'))
                self.assertEqual(data['assets/' + dependency.removeprefix('./')], b'geometry')
            self.assertEqual((part / 'shape.gltf').read_bytes(), raw)
            self.assertEqual((part / 'shape.glb').read_bytes(), glb)
            (part / 'mesh data.bin').unlink()
            out.write_bytes(b'keep output')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_html_video_assets_follow_the_chapter_and_skip_script_examples(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-html-media-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (root / 'clip.mp4').write_bytes(b'wrong')
            (part / 'clip.mp4').write_bytes(b'local video')
            (part / 'poster.svg').write_bytes(b'<svg/>')
            (part / 'model.obj').write_bytes(b'v 0 0 0\n')
            source = '<p>first\rsecond</p>\n<video src="./clip.mp4" poster="poster.svg" controls></video><script>let s="<img src=\'ignore.png\'>";</script>'
            body = ('```mdxb-html\n' + json.dumps({'source': source}) + '\n```\n\n'
                    + '<video controls\n src="clip.mp4"></video>\n\n<model-viewer src="./model.obj"></model-viewer>\n\n<canvas></canvas>\n<script>let x="<img src=\'example.png\'>"</script>\n\n'
                    + '```html\n<video src="missing.mp4">\n```\n')
            (part / 'chapter.md').write_text('# Story\n\n' + body, encoding='utf-8')
            out = root / 'result.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            data = {base64.b64decode(v['data']): v['mime'] for v in book['assets'].values()}
            self.assertEqual(data[b'local video'], 'video/mp4')
            self.assertEqual(data[b'<svg/>'], 'image/svg+xml')
            self.assertEqual(data[b'v 0 0 0\n'], 'model/obj')
            self.assertIn("ignore.png", book['chapters'][0]['content'])
            self.assertIn("example.png", book['chapters'][0]['content'])
            self.assertIn('<video controls\n src="./assets/', book['chapters'][0]['content'])
            self.assertIn('missing.mp4', book['chapters'][0]['content'])
            (part / 'clip.mp4').unlink()
            out.write_bytes(b'keep output')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_canvas_scripts_and_styles_use_chapter_relative_files(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-game-assets-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (root / 'game.js').write_text('wrong', encoding='utf-8')
            (part / 'game.js').write_text('local game', encoding='utf-8')
            (part / 'game.css').write_text('canvas { color: red; }', encoding='utf-8')
            source = '<canvas id="game"></canvas>\n<link href="./game.css"\n rel="stylesheet">\n<script src="game.js"></script>\n'
            (part / 'chapter.md').write_text('# Game\n\n' + source, encoding='utf-8')
            out = root / 'result.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            data = {base64.b64decode(v['data']): v['mime'] for v in book['assets'].values()}
            self.assertEqual(data[b'local game'], 'text/javascript')
            self.assertEqual(data[b'canvas { color: red; }'], 'text/css')
            self.assertNotIn(b'wrong', data)
            self.assertIn('<script src="./assets/', book['chapters'][0]['content'])
            self.assertIn('<link href="./assets/', book['chapters'][0]['content'])
            (part / 'game.js').unlink()
            out.write_bytes(b'keep output')
            failed = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_stylesheet_dependencies_are_embedded_and_confined(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-css-pack-') as tmp:
            root = Path(tmp)
            (root / 'part/styles').mkdir(parents=True)
            (root / 'part/images').mkdir()
            (root / 'part/images/a b.svg').write_bytes(b'<svg/>')
            (root / 'part/styles/font.woff2').write_bytes(b'font data')
            (root / 'part/styles/theme.css').write_text('@font-face { src: url(font.woff2) }', encoding='utf-8')
            css = r'''/* url(fake.png) */ @import url("./theme.css"); canvas { background: url('../images/a\20 b.svg#icon'); content: 'url(no.png)' }'''
            stylesheet = root / 'part/styles/game.css'
            stylesheet.write_text(css, encoding='utf-8')
            (root / 'part/chapter.md').write_text('# Game\n\n<canvas></canvas>\n<link rel="stylesheet" href="./styles/game.css">\n', encoding='utf-8')
            out = root / 'result.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            styles = [base64.b64decode(a['data']).decode('utf-8') for a in book['assets'].values() if a['mime'] == 'text/css']
            main = next(s for s in styles if 'canvas' in s)
            self.assertIn('url(fake.png)', main)
            self.assertIn("content: 'url(no.png)'", main)
            self.assertIn('data:image/svg+xml;base64,PHN2Zy8+#icon', main)
            imported = main.split('data:text/css;base64,', 1)[1].split('"', 1)[0]
            self.assertIn('data:font/woff2;base64,', base64.b64decode(imported).decode())
            self.assertEqual(stylesheet.read_text(encoding='utf-8'), css)
            (root / 'part/styles/theme.css').write_text("@import './game.css';", encoding='utf-8')
            out.write_bytes(b'keep output')
            failed = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_html_images_package_from_the_chapter_and_skip_literal_examples(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-html-images-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            (part / 'shots').mkdir(parents=True)
            (root / 'shots').mkdir()
            (part / 'shots/a&b.svg').write_bytes(b'chapter-image')
            (root / 'shots/a&b.svg').write_bytes(b'wrong-image')
            source = ('# Story\n\n<img data-src="missing.png"\n src="./shots/a&amp;b.svg" alt="cover" width="120">\n\n'
                      '| Shot | Frame |\n| --- | --- |\n| 1 | Left <img src=shots/a&amp;b.svg alt="detail"> right |\n\n'
                      '`<img src="missing1.png">`\n\n<!--\n<img src="missing2.png">\n![fake](missing3.png)\n-->\n\n'
                      '<a title="![fake](missing4.png)">text</a>\n\n\\<img src="missing5.png">\n\n'
                      '```html\n<img src="missing6.png">\n```\n\n'
                      'text <script>let a="![fake](missing7.png)"</script>\n\n'
                      '<script>\nlet a="![fake](missing8.png)";\n</script>\n')
            chapter = part / 'story.md'
            chapter.write_text(source, encoding='utf-8')
            out = root / 'book.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            content = book['chapters'][0]['content']
            self.assertEqual(content.count('./assets/'), 2)
            self.assertIn('data-src="missing.png"', content)
            self.assertIn('<img src="missing2.png">', content)
            self.assertIn('![fake](missing3.png)', content)
            self.assertIn('![fake](missing4.png)', content)
            self.assertIn('\\<img src="missing5.png">', content)
            self.assertIn('![fake](missing7.png)', content)
            self.assertIn('![fake](missing8.png)', content)
            self.assertEqual(base64.b64decode(next(iter(book['assets'].values()))['data']), b'chapter-image')
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            (part / 'shots/a&b.svg').unlink()
            out.write_bytes(b'keep output')
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_inline_css_uses_chapter_directory_and_preserves_source(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-inline-css-') as tmp:
            root = Path(tmp)
            part = root / 'part'
            part.mkdir()
            (root / 'frame.svg').write_bytes(b'wrong')
            (part / 'frame.svg').write_bytes(b'<svg/>')
            (part / 'theme.css').write_text('canvas { background-image: url(frame.svg) }', encoding='utf-8')
            source = ('# Game\n\n<canvas style="background:url(\'./frame.svg\')"></canvas>\n'
                      '<style>@import "theme.css"; canvas { border-image: url(frame.svg) }</style>\n'
                      '<!-- url(missing.png) --><script>let s="url(missing.png)"</script>\n')
            chapter = part / 'chapter.md'
            chapter.write_text(source, encoding='utf-8')
            out = root / 'result.mdxb'
            cmd = [sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            content = json.loads(out.read_text(encoding='utf-8'))['chapters'][0]['content']
            self.assertEqual(content.count('data:image/svg+xml;base64,PHN2Zy8+'), 2)
            imported = content.split('data:text/css;base64,', 1)[1].split('"', 1)[0]
            self.assertIn('data:image/svg+xml;base64,PHN2Zy8+', base64.b64decode(imported).decode())
            self.assertIn('url(missing.png)', content)
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)
            (part / 'frame.svg').unlink()
            out.write_bytes(b'keep output')
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(out.read_bytes(), b'keep output')

    def test_nested_chapters_and_translation_use_their_own_directory(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-relative-images-') as tmp:
            root = Path(tmp)
            chapter = root / 'part'
            (chapter / 'assets').mkdir(parents=True)
            (root / 'assets').mkdir()
            (chapter / 'assets' / '图 1.svg').write_bytes(b'local-image')
            (root / 'assets' / '图 1.svg').write_bytes(b'root-image')
            (chapter / 'english.png').write_bytes(b'english-image')
            text = ('# Storyboard\n\n| Shot | Frame |\n| --- | --- |\n'
                    '| 01 | ![Frame](./assets/%E5%9B%BE%201.svg "title") |\n'
                    '| 02 | ![Frame](assets/%E5%9B%BE%201.svg) |\n\n'
                    '`![literal](missing.png)`\n\n```md\n![example](missing.png)\n```\n')
            (chapter / '01.md').write_text(text, encoding='utf-8')
            (chapter / '01.en.md').write_text('# English\n\n![English](./english.png)\n', encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            self.assertEqual((chapter / '01.md').read_text(encoding='utf-8'), text)
            self.assertIn('![literal](missing.png)', book['chapters'][0]['content'])
            self.assertIn('![example](missing.png)', book['chapters'][0]['content'])
            local = next(key for key, asset in book['assets'].items() if base64.b64decode(asset['data']) == b'local-image')
            english = next(key for key, asset in book['assets'].items() if base64.b64decode(asset['data']) == b'english-image')
            self.assertEqual(book['chapters'][0]['content'].count('./' + local), 2)
            self.assertIn('"title"', book['chapters'][0]['content'])
            self.assertIn('./' + english, book['chapters'][0]['content_en'])

    def test_stylesheet_reuses_existing_root_assets_with_explicit_dot_path(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-css-existing-') as tmp:
            root = Path(tmp)
            (root / 'assets').mkdir()
            (root / 'assets/a b.svg').write_bytes(b'<svg/>')
            (root / 'game.css').write_text("canvas { background: url('./assets/a%20b.svg') }", encoding='utf-8')
            (root / 'game.md').write_text('# Game\n\n<canvas></canvas>\n<link rel="stylesheet" href="./game.css">\n', encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            styles = [base64.b64decode(a['data']) for a in book['assets'].values() if a['mime'] == 'text/css']
            self.assertTrue(any(b'data:image/svg+xml;base64,PHN2Zy8+' in css for css in styles))

    def test_missing_or_external_images_do_not_replace_output(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-missing-images-') as tmp:
            root = Path(tmp)
            book = root / 'book'
            book.mkdir()
            (root / 'outside.png').write_bytes(b'outside')
            out = root / 'result.mdxb'
            for reference in ('./missing.png', '../outside.png'):
                (book / 'chapter.md').write_text(f'# T\n\n![image]({reference})\n', encoding='utf-8')
                out.write_bytes(b'keep existing output')
                result = subprocess.run([sys.executable, str(TOOL), 'pack', str(book), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(out.read_bytes(), b'keep existing output')

    def test_xml_code_and_remote_images_are_not_rewritten(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-xml-images-') as tmp:
            root = Path(tmp)
            content = ('<title>Test</title><interactive type="canvas"><![CDATA['
                       '<img src="missing.png"><script>1+1</script>]]></interactive>'
                       '<!-- <img src="missing.png"> -->'
                       '<img src="https://example.test/image.png?x=1&amp;y=2"/>')
            (root / 'chapter.xml').write_text(content, encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(json.loads(out.read_text(encoding='utf-8'))['chapters'][0]['content'], content)

    def test_docxxml_html_widget_assets_are_embedded_but_script_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-xml-media-') as tmp:
            root = Path(tmp)
            (root / 'clip.mp4').write_bytes(b'local video')
            content = '<title>Test</title><interactive type="html"><![CDATA[<video src="./clip.mp4"></video><script>let s="<img src=\'missing.png\'>"</script>]]></interactive>'
            (root / 'chapter.xml').write_text(content, encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            self.assertIn('assets/', book['chapters'][0]['content'])
            self.assertIn('missing.png', book['chapters'][0]['content'])
            self.assertEqual(base64.b64decode(next(iter(book['assets'].values()))['data']), b'local video')

    def test_indented_code_does_not_load_assets_and_footnote_images_do(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-indented-code-') as tmp:
            root = Path(tmp)
            source = ('# Demo\n\n    ![literal](missing.png)\n    ![reference][shot]\n\n'
                      '[^a]: note\n\n    ![real](./note.svg)\n\n'
                      '        ![nested](nested-missing.png)\n\n- List\n\n'
                      '  ![list](note.svg)\n\n      ![code](list-missing.png)\n\n'
                      '> Quote\n>\n>     ![quoted](quoted-missing.png)\n\n'
                      '  ```md\n> ![fenced](fenced-missing.png)\n  ```\n\n'
                      '-     ![marker](marker-missing.png)\n      next line\n\n'
                      '[shot]: note.svg\n')
            chapter = root / 'chapter.md'
            chapter.write_text(source, encoding='utf-8')
            (root / 'note.svg').write_text('<svg/>', encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(out.read_text(encoding='utf-8'))
            content = book['chapters'][0]['content']
            self.assertTrue(content.startswith('    ![literal](missing.png)\n    ![reference][shot]'))
            self.assertIn('        ![nested](nested-missing.png)', content)
            self.assertIn('      ![code](list-missing.png)', content)
            self.assertIn('>     ![quoted](quoted-missing.png)', content)
            self.assertIn('> ![fenced](fenced-missing.png)', content)
            self.assertIn('-     ![marker](marker-missing.png)', content)
            self.assertEqual(len(book['assets']), 1)
            self.assertEqual(content.count('./assets/'), 2)
            self.assertEqual(chapter.read_text(encoding='utf-8'), source)

    def test_later_heading_and_code_example_are_not_removed_as_title(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-heading-code-') as tmp:
            root = Path(tmp)
            source = '```markdown\n# example heading\n```\n\nIntroduction\n\n# Later heading\n'
            (root / 'chapter.md').write_text(source, encoding='utf-8')
            out = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(TOOL), 'pack', str(root), '-o', str(out)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            chapter = json.loads(out.read_text(encoding='utf-8'))['chapters'][0]
            self.assertEqual(chapter['title']['en'], 'chapter')
            self.assertEqual(chapter['content'], source)

    @unittest.skipUnless(shutil.which('node'), 'Node.js is required for browser-reader checks')
    def test_reader_resolves_embedded_images_inside_tables_and_text(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-reader-images-') as tmp:
            temp = Path(tmp)
            (temp / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('markdown.js', 'docxml.js', 'katex.mjs', 'mdxb.js'):
                shutil.copyfile(ROOT / 'web' / 'js' / name, temp / name)
            script = temp / 'check.mjs'
            script.write_text(r'''
import assert from 'node:assert/strict';
import {normalizeBook, assetURL} from './mdxb.js';
import {parseMarkdown} from './markdown.js';
const assets = {'assets/图 1.svg': {mime:'image/svg+xml', data:'PHN2Zy8+'}};
const book = normalizeBook({format:'markdownx.book', version:1, assets,
  chapters:[{id:'one', format:'markdown', content:'Text ![pic](./assets/%E5%9B%BE%201.svg)\n\nShot | Image\n--- | ---\n01 | ![pic](assets/%E5%9B%BE%201.svg)\n'}]});
const blocks = book.chapters[0].blocks;
assert.match(blocks[0].html, /src="data:image\/svg\+xml;base64,PHN2Zy8\+"/);
assert.equal(blocks[1].type, 'table');
assert.match(blocks[1].rows[1][1].html, /src="data:image\/svg\+xml;base64,PHN2Zy8\+"/);
assert.equal(assetURL(book, './assets/%E5%9B%BE%201.svg'), 'data:image/svg+xml;base64,PHN2Zy8+');
assert.equal(assetURL(book, '../assets/%E5%9B%BE%201.svg'), '');
assert.equal(assetURL(book, 'javascript:alert(1)'), '');
const parsed = parseMarkdown('| A | B |\n| --- | --- |\n| a\\|b | `![x](bad)` |');
assert.equal(parsed.blocks[0].rows[1][0].html, 'a|b');
assert.match(parsed.blocks[0].rows[1][1].html, /^<code>/);
assert.doesNotMatch(parsed.blocks[0].rows[1][1].html, /<img/);
const parensAssets = {'assets/shot(1).svg': {mime:'image/svg+xml', data:'PHN2Zy8+'}};
const parensBook = normalizeBook({format:'markdownx.book', version:1, assets:parensAssets,
  chapters:[{id:'one',format:'markdown',content:'![frame [one]](./assets/shot(1).svg "caption")\n\n| A | B |\n|---|---|\n| 1 | before ![pic](assets/shot\\(1\\).svg) after |'}]});
assert.equal(parensBook.chapters[0].blocks[0].src, './assets/shot(1).svg');
assert.equal(parensBook.chapters[0].blocks[0].caption, 'frame [one]');
assert.match(parensBook.chapters[0].blocks[1].rows[1][1].html, /src="data:image\/svg\+xml;base64,PHN2Zy8\+"/);
const literals = parseMarkdown('\\![no](assets/shot(1).svg) <!-- ![no](assets/shot(1).svg) --> <a title="![no](assets/shot(1).svg)">x</a>', '', (s) => assetURL(parensBook, s));
assert.doesNotMatch(literals.blocks[0].html, /<img/);
const refs = normalizeBook({format:'markdownx.book', version:1, assets:parensAssets,
  chapters:[{id:'one', format:'markdown', content:'![cover][ SHOT ]\n\n| A | B |\n|---|---|\n| 1 | before ![detail][] after |\n\n[shot]: ./assets/shot(1).svg\n[detail]: assets/shot(1).svg "left|right"\n\n`![code][shot]`\n\ntext <!--\n[fake]: missing.png\n![fake][shot]\n-->'}]});
assert.equal(refs.chapters[0].blocks[0].type, 'image');
assert.equal(refs.chapters[0].blocks[0].src, './assets/shot(1).svg');
assert.match(refs.chapters[0].blocks[1].rows[1][1].html, /src="data:image\/svg\+xml;base64,PHN2Zy8\+"/);
assert.doesNotMatch(refs.chapters[0].blocks[2].html, /<img/);
assert.doesNotMatch(refs.chapters[0].blocks[3].html, /<img/);
const links = parseMarkdown('Source [paper][REF] and [site][].\n\n[ref]: https://example.test/paper(1) "Paper"\n[site]: https://example.test/\n\n[local file][file]\n\n[file]: ./paper.pdf\n\n`[example][ref]`');
assert.match(links.blocks[0].html, /href="https:\/\/example\.test\/paper%281%29"/);
assert.match(links.blocks[0].html, /href="https:\/\/example\.test\/"/);
assert.ok(links.blocks.some(b => b.html?.includes('[file]: ./paper.pdf')));
assert.ok(links.blocks.some(b => b.html?.includes('[local file][file]')));
assert.ok(links.blocks.some(b => b.html?.includes('<code>[example][ref]</code>')));
const imageAndLocalLink = parseMarkdown('![frame][same]\n\n[download][same]\n\n[same]: ./assets/shot(1).svg');
assert.equal(imageAndLocalLink.blocks[0].type, 'image');
assert.ok(imageAndLocalLink.blocks.some(b => b.html?.includes('[same]: ./assets/shot(1).svg')));
const linkedBook = normalizeBook({format:'markdownx.book',version:2,assets:{},chapters:[
  {id:'first',source_path:'part/first.md',source_en_path:'en/first.md',format:'markdown',content:'[Next](./第二%20章.md) and [ref][next] and [Heading](./第二%20章.md#目标-标题) and [Here](#目标)\n\n[next]: 第二%20章.md',content_en:'[Next](../part/第二%20章.md)'},
  {id:'second',source_path:'part/第二 章.md',format:'markdown',content:'[Back](first.md)'}]});
assert.equal((linkedBook.chapters[0].blocks[0].html.match(/href="#mdxb-chapter\/second"/g) || []).length, 2);
assert.match(linkedBook.chapters[0].blocks[0].html, /href="#mdxb-chapter\/second\?anchor=%E7%9B%AE%E6%A0%87-%E6%A0%87%E9%A2%98"/);
assert.match(linkedBook.chapters[0].blocks[0].html, /href="#mdxb-chapter\/first\?anchor=%E7%9B%AE%E6%A0%87"/);
assert.match(linkedBook.chapters[0].blocksEn[0].html, /href="#mdxb-chapter\/second"/);
assert.match(linkedBook.chapters[1].blocks[0].html, /href="#mdxb-chapter\/first"/);
const invalidLinks = normalizeBook({format:'markdownx.book',version:1,assets:{},chapters:[
  {id:'one',source_path:'folder/one.md',format:'markdown',content:'[outside](../../outside.md)'},
  {id:'two',source_path:'outside.md',format:'markdown',content:'Nope'}]});
assert.doesNotMatch(invalidLinks.chapters[0].blocks[0].html, /mdxb-chapter/);
const raw = parseMarkdown('<video controls\n src="./clip.mp4"></video>\n\n<canvas></canvas>\n<script>let n=1;</script>\n\n```html\n<video src="example.mp4"></video>\n```');
assert.deepEqual(raw.blocks.map(b => b.type), ['interactive', 'interactive', 'code']);
assert.match(raw.blocks[0].source, /clip\.mp4/);
assert.match(raw.blocks[1].source, /let n=1/);
const htmlImages = parseMarkdown('<img\n src="./assets/%E5%9B%BE%201.svg" alt="封面" width="320">\n\n| A | B |\n| --- | --- |\n| 1 | Left <img data-src="bad" src=assets/%E5%9B%BE%201.svg alt="a &amp; b" onerror="bad()"> right |\n\n`<img src="bad">`\n\n<!-- <img src="bad"> -->', '', (s) => assetURL(book, s));
assert.equal(htmlImages.blocks[0].type, 'image');
assert.equal(htmlImages.blocks[0].width, 320);
assert.match(htmlImages.blocks[1].rows[1][1].html, /src="data:image\/svg\+xml;base64,PHN2Zy8\+"/);
assert.match(htmlImages.blocks[1].rows[1][1].html, /alt="a &amp; b"/);
assert.doesNotMatch(htmlImages.blocks[1].rows[1][1].html, /onerror/);
assert.doesNotMatch(htmlImages.blocks[2].html, /<img/);
assert.doesNotMatch(htmlImages.blocks[3].html, /<img/);
const unsafe = normalizeBook({format:'markdownx.book', version:1,
  assets:{'assets/a.png':{mime:'image/png" onerror="alert(1)',data:'AAAA'}},
  chapters:[{id:'one',format:'markdown',content:'text ![" onerror="x](assets/a.png)'}]});
assert.doesNotMatch(unsafe.chapters[0].blocks[0].html, /" onerror="/);
''', encoding='utf-8')
            result = subprocess.run(['node', str(script)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
