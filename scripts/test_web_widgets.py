"""Check animation scheduling and widget cleanup without a browser dependency."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required for widget checks')
class WebWidgetTests(unittest.TestCase):
    def test_markdown_table_video_and_encoded_multiline_canvas(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-table-widget-') as tmp:
            temp_dir = Path(tmp)
            (temp_dir / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('docxml.js', 'katex.mjs', 'markdown.js'):
                shutil.copyfile(ROOT / 'web' / 'js' / name, temp_dir / name)
            script = temp_dir / 'test.mjs'
            script.write_text(r'''
import assert from 'node:assert/strict';
import {parseMarkdown} from './markdown.js';
const source = '<canvas id="g"></canvas>\n<script>let title = "中文 | game";</script>';
const payload = Buffer.from(JSON.stringify({kind:'html', title:'game', height:240, source})).toString('base64');
const text = '| Shot | Media |\n| --- | --- |\n| 1 | <video controls src="./clip.mp4"></video> |\n| 2 | <markdownx-interactive data-base64="' + payload + '"></markdownx-interactive> |';
const table = parseMarkdown(text).blocks[0];
assert.equal(table.rows[1][1].blocks[0].source, '<video controls src="./clip.mp4"></video>');
assert.equal(table.rows[2][1].blocks[0].source, source);
assert.equal(table.rows[2][1].blocks[0].height, 240);
const wrapped = "<div class='game'>\n<!-- </div><video src='fake.mp4'></video> -->\n<div><canvas></canvas></div>\n<script>const sample='</div>';</script>\n</div>\n<script src='./game.js'></script>\n\nAfter";
const blocks = parseMarkdown(wrapped).blocks;
assert.equal(blocks[0].type, 'interactive');
assert.ok(blocks[0].source.includes("sample='</div>'") && blocks[0].source.includes('./game.js'));
assert.equal(blocks[1].html, 'After');
const wrappedTable = parseMarkdown("| Shot | Game |\n| --- | --- |\n| 1 | <div><video src='clip.mp4'></video></div> |").blocks[0];
assert.equal(wrappedTable.rows[1][1].blocks[0].type, 'interactive');
for (const inert of ["<div>plain</div>", "<div><!-- <canvas></canvas> --></div>", "<div><script>let s='<canvas>';</script></div>", "<div><script>let s='</scripture><canvas></canvas>';</script></div>", "<div><canvas></canvas></div> trailing"]) {
  assert.ok(!parseMarkdown(inert).blocks.some(b => b.type === 'interactive'), inert);
}
''', encoding='utf-8')
            result = subprocess.run(['node', str(script)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_projectile_schedules_one_frame_and_disconnects_observers(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-widget-') as tmp:
            temp_dir = Path(tmp)
            script = temp_dir / 'test.mjs'
            (temp_dir / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('widgets.js', 'docxml.js', 'katex.mjs', 'mdxb.js', 'markdown.js'):
                shutil.copyfile(ROOT / 'web' / 'js' / name, temp_dir / name)
            script.write_text(r'''
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
let pending = new Map(), serial = 0, resizeStopped = 0, mutationStopped = 0;
globalThis.requestAnimationFrame = fn => { const id = ++serial; pending.set(id, fn); return id; };
globalThis.cancelAnimationFrame = id => pending.delete(id);
function frame(time) { const tasks = [...pending.values()]; pending.clear(); tasks.forEach(fn => fn(time)); }
globalThis.ResizeObserver = class { observe() {} disconnect() { resizeStopped++; } };
globalThis.MutationObserver = class { observe() {} disconnect() { mutationStopped++; } };
globalThis.window = {devicePixelRatio: 1};
globalThis.getComputedStyle = () => ({getPropertyValue: () => ''});
const ctx = new Proxy({}, {get(target, key) { return target[key] ?? (() => {}); }, set(target, key, value) { target[key] = value; return true; }});
class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.style = {}; this.dataset = {}; this.attributes = {}; this.listeners = {}; this.clientWidth = 300; this.className = ''; if (tag === 'template') this.content = {childNodes: [], querySelectorAll: () => []}; }
  appendChild(child) { this.children.push(child); return child; }
  append(...children) { this.children.push(...children); }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(name, callback) { this.listeners[name] = callback; }
  getContext() { return ctx; }
  querySelectorAll(selector) { let found = []; for (const child of this.children) { if (selector === '.widget' && child.className === 'widget') found.push(child); found.push(...child.querySelectorAll(selector)); } return found; }
}
globalThis.document = {documentElement: {lang: 'en'}, createElement: tag => new Element(tag)};
const {mountInteractive, disposeInteractive} = await import(pathToFileURL(process.argv[2]).href);
const {rewriteSrcset} = await import(pathToFileURL(process.argv[2]).href);
const {assetURL, mediaAssetURL, scriptAssetURL, pdfAssetLink} = await import(pathToFileURL(process.argv[2].replace('widgets.js', 'mdxb.js')).href);
const resourceBook = {assets:{'assets/frame.svg':{mime:'image/svg+xml',data:'PHN2Zy8+'}}};
assert.equal(assetURL(resourceBook, './assets/frame.svg?v=1'), 'data:image/svg+xml;base64,PHN2Zy8+');
assert.equal(assetURL(resourceBook, 'assets/frame.svg?cache=2'), 'data:image/svg+xml;base64,PHN2Zy8+');
assert.equal(assetURL(resourceBook, './assets/frame.svg?v=1#icon'), 'data:image/svg+xml;base64,PHN2Zy8+#icon');
assert.equal(assetURL(resourceBook, './assets/frame.svg#svgView(viewBox(50,0,50,50))'), 'data:image/svg+xml;base64,PHN2Zy8+#svgView(viewBox(50,0,50,50))');
assert.equal(assetURL(resourceBook, '../assets/frame.svg#icon'), '');
const namedHash = {assets:{'assets/frame#take.svg':{mime:'image/svg+xml',data:'PHN2Zy8+'}}};
assert.equal(assetURL(namedHash, './assets/frame%23take.svg#icon'), 'data:image/svg+xml;base64,PHN2Zy8+#icon');
assert.equal(assetURL({assets:{'assets/frame.png':{mime:'image/png',data:'QUJD'}}}, './assets/frame.png#icon'), '');
const namedQuery = {assets:{'assets/frame?name.svg':{mime:'image/svg+xml',data:'PHN2Zy8+'}}};
assert.equal(assetURL(namedQuery, './assets/frame%3Fname.svg'), 'data:image/svg+xml;base64,PHN2Zy8+');
const mediaBook = {assets:{'assets/clip.mp4':{mime:'video/mp4',data:'QUJD'}}};
const pdfBook = {assets:{'assets/论文 1.pdf':{mime:'application/pdf',data:'JVBERi0xLjQ='}}};
assert.equal(pdfAssetLink(pdfBook, './assets/%E8%AE%BA%E6%96%87%201.pdf#page=002'), '#mdxb-pdf/assets%2F%E8%AE%BA%E6%96%87%201.pdf?page=2');
assert.equal(pdfAssetLink(pdfBook, '../assets/%E8%AE%BA%E6%96%87%201.pdf'), '');
assert.equal(pdfAssetLink(pdfBook, './assets/%E8%AE%BA%E6%96%87%201.pdf#page=0'), '');
assert.equal(pdfAssetLink(resourceBook, './assets/frame.svg#page=2'), '');
assert.equal(mediaAssetURL(mediaBook, './assets/clip.mp4?v=1#t=2,5'), 'data:video/mp4;base64,QUJD#t=2,5');
assert.equal(mediaAssetURL(mediaBook, './assets/clip.mp4#t=npt:,5'), 'data:video/mp4;base64,QUJD#t=npt:,5');
assert.equal(mediaAssetURL(mediaBook, './assets/clip.mp4#t=2&bad'), '');
assert.equal(mediaAssetURL(resourceBook, './assets/frame.svg#t=2'), '');
const scriptBook = {assets:{'assets/game.mjs':{mime:'text/javascript',data:'QUJD'}}};
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs?v=1', true), 'data:text/javascript;base64,QUJD#%3F%76%3D%31');
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs?v=中', true), 'data:text/javascript;base64,QUJD#%3F%76%3D%25%45%34%25%42%38%25%41%44');
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs?v=中', true), scriptAssetURL(scriptBook, './assets/game.mjs?v=%E4%B8%AD', true));
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs#%3F%76%3D%E4%B8%AD', true), scriptAssetURL(scriptBook, './assets/game.mjs?v=中', true));
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs#%3F%76%3D%31', true), 'data:text/javascript;base64,QUJD#%3F%76%3D%31');
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs?v=2', false), 'data:text/javascript;base64,QUJD');
assert.equal(scriptAssetURL(scriptBook, './assets/game.mjs#unsafe', true), '');
assert.equal(scriptAssetURL(resourceBook, './assets/frame.svg?v=1', true), '');
const paths = {'./small.svg':'data:image/svg+xml;base64,AAAA', 'big.svg':'data:image/svg+xml;base64,QkJC'};
assert.equal(rewriteSrcset('./small.svg 1x, big.svg 2x', src => paths[src] || ''), 'data:image/svg+xml;base64,AAAA 1x, data:image/svg+xml;base64,QkJC 2x');
assert.equal(rewriteSrcset('data:image/png;base64,AAAA 1x, big.svg 2x', src => paths[src] || src), 'data:image/png;base64,AAAA 1x, data:image/svg+xml;base64,QkJC 2x');
const host = new Element('div');
const widget = mountInteractive({kind:'canvas', title:'test', source:'{"kind":"projectile","angle":45,"speed":14}'}, {}, 'en');
host.appendChild(widget); frame(10);
const controls = widget.children[0].children[1];
const button = controls.children.find(child => child.tag === 'button');
assert.ok(button); button.listeners.click();
assert.equal(pending.size, 1); frame(100); assert.equal(pending.size, 1);
frame(850); assert.equal(button.textContent, 'Flying…'); assert.equal(pending.size, 1);
frame(2200); assert.equal(button.textContent, 'Launch again'); assert.equal(pending.size, 0);
button.listeners.click(); frame(2300); disposeInteractive(host);
assert.equal(pending.size, 0); assert.equal(resizeStopped, 1); assert.equal(mutationStopped, 1);
const html = mountInteractive({kind:'html', title:'test', height:200, source:'<script>1+1</script>'}, {}, 'en');
assert.equal(html.children[0].attributes.sandbox, 'allow-scripts');
assert.match(html.children[0].srcdoc, /connect-src data:/);
''', encoding='utf-8')
            result = subprocess.run(['node', str(script), str(temp_dir / 'widgets.js')],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
