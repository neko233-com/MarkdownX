"""Check offline mathematical rendering and source preservation in the book reader."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(shutil.which('node'), 'Node.js is required for reader checks')
class WebMathTests(unittest.TestCase):
    def test_nested_formulas_display_math_tables_and_inert_input(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-web-math-') as tmp:
            root = Path(tmp)
            (root / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            for name in ('docxml.js', 'markdown.js', 'katex.mjs'):
                shutil.copyfile(ROOT / 'web/js' / name, root / name)
            script = root / 'test.mjs'
            script.write_text(r'''
import assert from 'node:assert/strict';
import {parseMarkdown} from './markdown.js';
import {mathHTML} from './docxml.js';
const source = String.raw`# 公式 $E=mc^2$

行内 $\frac{1}{\frac{a_i}{b}}$，以及 $\sum_{i=1}^{n} i$。公式($1+2$)。

$$
\int_0^1 \frac{x^2}{1+x} \, dx
$$

$$\sqrt{a^2+b^2}$$

| 项 | 公式 |
| --- | --- |
| 1 | $\frac{a}{b}$ |

代码 ` + '`$not_math$`' + String.raw` 和 \$escaped$；价格 $5 and $10 或 $5-$10.

引用[^a]

[^a]: 公式 $x_i^2$。

~~~text
$$x^2$$
~~~`;
const blocks = parseMarkdown(source).blocks;
const heading = blocks.find(b => b.type === 'h');
assert.equal(heading.anchorHtml, '公式 E=mc^2');
assert.match(heading.html, /<math/);
const paragraph = blocks.find(b => b.type === 'p' && b.html.includes('行内'));
assert.equal((paragraph.html.match(/<mfrac>/g) || []).length, 2);
assert.match(paragraph.html, /<munderover>|<msubsup>/);
assert.match(paragraph.html, /<msub>/);
assert.equal((paragraph.html.match(/<math/g) || []).length, 3);
const display = blocks.filter(b => b.type === 'p' && b.html.includes('math-display'));
assert.equal(display.length, 2);
assert.match(display[0].html, /<mfrac>/);
assert.match(display[1].html, /<msqrt>/);
const table = blocks.find(b => b.type === 'table');
assert.match(table.rows[1][1].html, /<mfrac>/);
const code = blocks.find(b => b.type === 'code');
assert.equal(code.code, '$$x^2$$');
const literal = blocks.find(b => b.type === 'p' && b.html.includes('价格'));
assert.match(literal.html, /<code>\$not_math\$<\/code>/);
assert.ok(literal.html.includes('$5 and $10') && literal.html.includes('$5-$10'));
assert.ok(literal.html.includes('$escaped$') && !literal.html.includes(String.raw`\$escaped`));
assert.doesNotMatch(literal.html, /<math/);
const footnote = blocks.find(b => b.type === 'footnotes').notes[0].blocks[0].html;
assert.match(footnote, /<msubsup>/);
const invalid = mathHTML(String.raw`\unknown{<img src=x onerror=alert(1)>}`);
assert.match(invalid, /math-fallback/);
assert.doesNotMatch(invalid, /<img/);
assert.match(invalid, /&lt;img/);
assert.match(mathHTML('x'.repeat(16385)), /math-fallback/);
let resolved = [];
const resolver = src => { resolved.push(src); return 'https://example.test/' + src; };
resolver.link = () => '';
const mixed = parseMarkdown(String.raw`$\text{![fake](missing.png)}$ and $\text{![fake][shot]}$ and $\text{[^note]}$.

$$
\text{![block][shot]}
$$

> $$
> \text{![quote][shot]}
> $$

[shot]: missing.png

[^note]: real note

[公式 $x_i^2$](https://example.test/paper) and [![cover](real.svg)](https://example.test/cover).`, '', resolver).blocks;
const mixedHtml = JSON.stringify(mixed);
assert.match(mixedHtml, /!\[fake\]\[shot\]/);
assert.match(mixedHtml, /!\[block\]\[shot\]/);
assert.match(mixedHtml, /!\[quote\]\[shot\]/);
assert.doesNotMatch(JSON.stringify(mixed.slice(0, 2)), /<img /);
assert.deepEqual(resolved, ['real.svg']);
assert.ok(!mixed.some(b => b.type === 'footnotes'));
const linked = mixed.find(b => b.type === 'p' && b.html.includes('example.test/paper')).html;
assert.match(linked, /<a[^>]+>公式 <span class="math-inline"><span class="katex"><math/);
assert.match(linked, /<a[^>]+><img class="inline-image"/);
assert.doesNotMatch(linked, /\u0000\d+\u0000/);
const raw = parseMarkdown('$$\nunclosed').blocks;
assert.ok(JSON.stringify(raw).includes('$$'));
''', encoding='utf-8')
            result = subprocess.run(['node', str(script)], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
