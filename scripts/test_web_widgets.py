"""Check animation scheduling and widget cleanup without a browser dependency."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required for widget checks')
class WebWidgetTests(unittest.TestCase):
    def test_projectile_schedules_one_frame_and_disconnects_observers(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-widget-') as tmp:
            temp_dir = Path(tmp)
            script = temp_dir / 'test.mjs'
            (temp_dir / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('widgets.js', 'docxml.js'):
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
  constructor(tag) { this.tag = tag; this.children = []; this.style = {}; this.dataset = {}; this.attributes = {}; this.listeners = {}; this.clientWidth = 300; this.className = ''; }
  appendChild(child) { this.children.push(child); return child; }
  append(...children) { this.children.push(...children); }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(name, callback) { this.listeners[name] = callback; }
  getContext() { return ctx; }
  querySelectorAll(selector) { let found = []; for (const child of this.children) { if (selector === '.widget' && child.className === 'widget') found.push(child); found.push(...child.querySelectorAll(selector)); } return found; }
}
globalThis.document = {documentElement: {lang: 'en'}, createElement: tag => new Element(tag)};
const {mountInteractive, disposeInteractive} = await import(pathToFileURL(process.argv[2]).href);
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
assert.match(html.children[0].srcdoc, /connect-src 'none'/);
''', encoding='utf-8')
            result = subprocess.run(['node', str(script), str(temp_dir / 'widgets.js')],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
