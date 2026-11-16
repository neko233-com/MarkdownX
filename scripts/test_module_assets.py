"""Package literal ES module dependencies without changing the source files."""
import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / 'tools/agent'))
from js_modules import module_imports, module_references  # noqa: E402


class ModuleAssetTests(unittest.TestCase):
    def test_script_dom_media_is_embedded_from_document_directory(self):
        self.assertEqual(module_references("const img=new Image(); img.src='frame.svg'; [1].map((img, i)=>img.src='unrelated.svg');"), [])
        local = "const fetch=x=>x; fetch('missing.json'); window.fetch('data.json'); class Image {} const img=new Image(); img.src='missing.svg';"
        self.assertEqual([(value, kind) for _, _, value, kind in module_references(local)], [('data.json', 'fetch')])
        semicolon_free = "import {value} from './helper.mjs'\nfetch('./data.json')"
        self.assertEqual([(value, kind) for _, _, value, kind in module_references(semicolon_free)], [('./helper.mjs', 'module'), ('./data.json', 'fetch')])
        parameters = "function f({fetch}) { fetch('missing.json'); } window.fetch('data.json'); const run=({Image})=>{const img=new Image();img.src='missing.svg';};"
        self.assertEqual([(value, kind) for _, _, value, kind in module_references(parameters)], [('data.json', 'fetch')])
        imported = "import Image from './image.mjs'; const img=new Image(); img.src='missing.svg';"
        self.assertEqual([(value, kind) for _, _, value, kind in module_references(imported)], [('./image.mjs', 'module')])
        self.assertEqual([(value, kind) for _, _, value, kind in module_references("const img=document.createElement('IMG');img.setAttribute('SRC','frame.svg');")], [('frame.svg', 'dom')])
        self.assertEqual(module_references("const img=new Image().other; img.src='missing.svg';"), [])
        self.assertEqual([(value, kind) for _, _, value, kind in module_references("const img=new Image()\nimg.src='frame.svg'")], [('frame.svg', 'dom')])
        with tempfile.TemporaryDirectory(prefix='markdownx-dom-media-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            scripts = chapter / 'scripts'
            scripts.mkdir(parents=True)
            (chapter / 'frame.svg').write_bytes(b'<svg/>')
            (scripts / 'frame.svg').write_bytes(b'wrong')
            (chapter / 'sound.mp3').write_bytes(b'sound')
            script = scripts / 'game.js'
            code = ("const img=new Image(); img.src='./frame.svg'; "
                    "const other=document.createElement('img'); other.setAttribute('src','frame.svg'); "
                    "const sound=new Audio('sound.mp3'); const ignored={}; ignored.src='missing.svg'; "
                    "const text=\"img.src='missing.svg'\"; function shadow(img){img.src='unrelated.svg';} img.src='./'+name;")
            script.write_text(code, encoding='utf-8')
            document = chapter / 'game.md'
            source = "# Game\n\n<canvas></canvas>\n<script src='./scripts/game.js'></script>\n"
            document.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            bundled = next(base64.b64decode(asset['data']).decode() for asset in book['assets'].values() if asset['mime'] in ('text/javascript', 'application/javascript'))
            self.assertEqual(bundled.count('data:image/svg+xml;base64,PHN2Zy8+'), 2)
            self.assertIn('data:audio/mpeg;base64,c291bmQ=', bundled)
            self.assertIn("ignored.src='missing.svg'", bundled)
            self.assertIn("img.src='./'+name", bundled)
            self.assertIn("img.src='unrelated.svg'", bundled)
            self.assertEqual(document.read_text(encoding='utf-8'), source)
            self.assertEqual(script.read_text(encoding='utf-8'), code)

    def test_classic_script_dynamic_import_is_bundled_from_script_directory(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-classic-import-') as tmp:
            root = Path(tmp)
            scripts = root / 'scripts'
            scripts.mkdir()
            (scripts / 'helper.mjs').write_text('export const n=42;', encoding='utf-8')
            (scripts / 'game.js').write_text("import('./helper.mjs').then(m=>console.log(m.n));", encoding='utf-8')
            (root / 'game.md').write_text("# Game\n\n<canvas></canvas>\n<script src='./scripts/game.js'></script>\n", encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            contents = [base64.b64decode(asset['data']).decode() for asset in book['assets'].values() if asset['mime'] in ('text/javascript', 'application/javascript')]
            self.assertTrue(any('data:text/javascript;base64,' in content for content in contents))

    def test_local_fetch_uses_document_directory_and_preserves_source(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-fetch-book-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            scripts = chapter / 'scripts'
            scripts.mkdir(parents=True)
            (chapter / 'data.json').write_text('{"place":"chapter"}', encoding='utf-8')
            (scripts / 'data.json').write_text('wrong', encoding='utf-8')
            (scripts / 'game.js').write_text("fetch('data.json').then(r=>r.json()); obj.fetch('missing.json'); fetch('https://example.invalid/x');", encoding='utf-8')
            source = ("# Game\n\n<canvas></canvas>\n<script src='./scripts/game.js'></script>\n"
                      "<script>window.fetch('./data.json');</script>\n"
                      "<script type='application/json'>{\"fetch\":\"missing.json\"}</script>\n")
            document = chapter / 'game.md'
            document.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            result = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(root), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            script = next(base64.b64decode(asset['data']).decode() for asset in book['assets'].values() if asset['mime'] in ('text/javascript', 'application/javascript'))
            encoded = base64.b64encode(b'{"place":"chapter"}').decode()
            self.assertIn(encoded, script)
            self.assertIn("obj.fetch('missing.json')", script)
            self.assertIn("fetch('https://example.invalid/x')", script)
            self.assertIn(encoded, book['chapters'][0]['content'])
            self.assertIn('{"fetch":"missing.json"}', book['chapters'][0]['content'])
            self.assertEqual(document.read_text(encoding='utf-8'), source)

    def test_lexer_skips_comments_strings_regex_and_properties(self):
        source = '''// import './fake.js'
const text = "import './fake.js'"; const regex = /import\\s+['\\"]fake/;
if (true) /import\\s+['\\"]fake/.test(text);
obj.import('./ignored.js'); const object = {import: 1, from: './ignored.js'}; import './a.js';
import { n } from '../b.js'; export * from './c.js';
async function f() { return import('./d.js'); }
const t = `literal import './fake.js' ${import('./e.js')}`;
'''
        self.assertEqual([value for _, _, value in module_imports(source)], ['./a.js', '../b.js', './c.js', './d.js', './e.js'])

    def test_module_relative_urls_and_inert_examples(self):
        source = ("const text=\"new URL('missing.svg', import.meta.url)\";\n"
                  "const a=new URL('frame.svg',import.meta.url); const b=new URL('./other.svg', import.meta.url);\n"
                  "const ignored=new URL('./no.svg', document.baseURI);")
        self.assertEqual([(value, kind) for _, _, value, kind in module_references(source)],
                         [('frame.svg', 'resource'), ('./other.svg', 'resource')])

    def test_module_resources_are_resolved_beside_the_importing_file(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-module-resource-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            (root / 'frame.svg').write_bytes(b'wrong')
            (chapter / 'frame.svg').write_bytes(b'<svg/>')
            module = chapter / 'game.mjs'
            module.write_text("const a=new URL('frame.svg',import.meta.url); const b=new URL('./frame.svg?v=1#icon',import.meta.url);", encoding='utf-8')
            source = "# Game\n\n<canvas></canvas>\n<script type='module' src='game.mjs'></script>\n"
            markdown = chapter / 'game.md'
            markdown.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            cmd = [sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(root), '-o', str(output)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            script = next(base64.b64decode(asset['data']).decode('utf-8') for asset in book['assets'].values() if asset['mime'] == 'text/javascript')
            self.assertEqual(script.count('data:image/svg+xml;base64,PHN2Zy8+'), 2)
            self.assertIn('#icon', script)
            self.assertNotIn('?v=1', script)
            self.assertEqual(markdown.read_text(encoding='utf-8'), source)
            output.write_bytes(b'keep')
            module.write_text("new URL('../private.svg',import.meta.url)", encoding='utf-8')
            private = root / 'private.svg'
            private.write_text('private', encoding='utf-8')
            denied = subprocess.run([sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(chapter), '-o', str(output)], capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(denied.returncode, 0)
            self.assertIn('outside', denied.stderr.lower())
            self.assertEqual(output.read_bytes(), b'keep')

    def test_pack_module_graph_and_reject_unsafe_or_circular_imports(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-module-book-') as tmp:
            root = Path(tmp)
            chapter = root / 'chapter'
            chapter.mkdir()
            helper = chapter / 'helper.mjs'
            helper.write_text('export const score = 42;', encoding='utf-8')
            (chapter / 'one.mjs').write_text('globalThis.count=(globalThis.count||0)+1;', encoding='utf-8')
            (chapter / 'two.mjs').write_text('globalThis.count=(globalThis.count||0)+1;', encoding='utf-8')
            entry = chapter / 'game.mjs'
            entry.write_text("import {score} from './helper.mjs?v=1'; import './one.mjs'; import './two.mjs'; document.body.dataset.score = score;", encoding='utf-8')
            source = ("# Game\n\n<canvas></canvas>\n<script type='module' src='game.mjs'></script>\n\n"
                      "<canvas></canvas>\n<script type='module'>import './helper.mjs';</script>\n\n"
                      "<canvas></canvas>\n<script>const example=\"import './missing.js'\";</script>\n")
            markdown = chapter / 'game.md'
            markdown.write_text(source, encoding='utf-8')
            output = root / 'book.mdxb'
            cmd = [sys.executable, str(ROOT / 'tools/agent/mdxb.py'), 'pack', str(chapter), '-o', str(output)]
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            book = json.loads(output.read_text(encoding='utf-8'))
            contents = [base64.b64decode(asset['data']).decode('utf-8') for asset in book['assets'].values() if asset['mime'] == 'text/javascript']
            bundled = next(text for text in contents if 'dataset.score' in text)
            self.assertIn('data:text/javascript;base64,', bundled)
            self.assertIn('#%3F%76%3D%31', bundled)
            identical_modules = [text for text in contents if 'globalThis.count' in text]
            self.assertEqual(len(identical_modules), 2)
            self.assertNotEqual(identical_modules[0], identical_modules[1])
            self.assertIn('data:text/javascript;base64,', book['chapters'][0]['content'])
            self.assertIn("import './missing.js'", book['chapters'][0]['content'])
            self.assertEqual(markdown.read_text(encoding='utf-8'), source)
            helper.write_text("import './game.mjs';", encoding='utf-8')
            output.write_bytes(b'keep')
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('circular', result.stderr.lower())
            self.assertEqual(output.read_bytes(), b'keep')
            helper.write_text("import '../private.mjs';", encoding='utf-8')
            (root / 'private.mjs').write_text('export const secret=1;', encoding='utf-8')
            result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('outside', result.stderr.lower())
            self.assertEqual(output.read_bytes(), b'keep')


if __name__ == '__main__':
    unittest.main()
