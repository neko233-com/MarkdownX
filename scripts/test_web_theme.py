"""Reader accent accessibility, including authored colors and malformed metadata."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required')
class WebThemeTests(unittest.TestCase):
    def test_authored_accents_remain_readable_without_mutating_metadata(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-colors-') as tmp:
            root = Path(tmp)
            shutil.copyfile(ROOT / 'web/js/theme.js', root / 'theme.mjs')
            script = root / 'test.mjs'
            script.write_text('''import assert from 'node:assert/strict';
import {readableAccent} from './theme.mjs';
const luminance = hex => [1,3,5].map(i=>parseInt(hex.slice(i,i+2),16)/255).reduce((s,v,i)=>s+[.2126,.7152,.0722][i]*(v<=.04045?v/12.92:((v+.055)/1.055)**2.4),0);
for (const dark of [false,true]) {
  for (const input of ['#b45309','#0070f3','#fff','#000','#ffffff','#000000','#ff0000','#00ff00','#0000ff',null,'bad','url(https://example.test)']) {
    const meta = Object.freeze({accent:input});
    const output = readableAccent(meta.accent,dark);
    assert.match(output,/^#[0-9a-f]{6}$/);
    for (const bg of dark ? ['#0a0a0a','#111111','#171717'] : ['#ffffff','#fafafa']) {
      const a=luminance(output), b=luminance(bg);
      assert.ok((Math.max(a,b)+.05)/(Math.min(a,b)+.05)>=4.5,`${input} -> ${output} on ${bg}`);
    }
    assert.equal(meta.accent,input);
  }
}
assert.equal(readableAccent('#b45309',false),'#b45309');
''', encoding='utf-8')
            subprocess.run(['node', str(script)], check=True, capture_output=True, text=True)
