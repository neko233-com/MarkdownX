#!/usr/bin/env python3
"""Build the three sample .mdxb books.

The output is checked in so the reader works from a plain clone. Run:

    python examples/build.py

The books are deliberately small: one idea per chapter, one widget to touch,
and a bilingual pair of bodies whenever it helps the demo.
"""
from __future__ import annotations

import json
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
NOW = int(time.time() * 1000)


def loc(zh: str, en: str) -> dict[str, str]:
    return {"zh": zh, "en": en}


def chapter(cid: str, title: tuple[str, str], content: str, content_en: str = "", fmt: str = "docxxml") -> dict:
    ch = {"id": cid, "title": loc(*title), "format": fmt, "content": content}
    if content_en:
        ch["content_en"] = content_en
    return ch


def book(bid: str, title: tuple[str, str], subtitle: tuple[str, str], accent: str, chapters: list[dict], authors: list[str]) -> dict:
    return {
        "format": "markdownx.book",
        "version": 1,
        "id": bid,
        "title": loc(*title),
        "subtitle": loc(*subtitle),
        "authors": authors,
        "languages": ["zh", "en"],
        "default_language": "zh",
        "cover": "",
        "theme": {"accent": accent, "background": "", "text": "", "font": "serif"},
        "reading": {"layout": "scroll", "max_width": 760, "show_toc": True},
        "chapters": chapters,
        "assets": {},
        "created": NOW,
        "modified": NOW,
        "generator": "MarkdownX examples/build.py",
    }


QUADRATIC_1 = '{"kind":"quadratic","a":1,"b":0,"c":0,"xMin":-4,"xMax":4,"yMin":-4,"yMax":8}'
QUADRATIC_2 = '{"kind":"quadratic","a":-0.5,"b":3,"c":1,"xMin":-3,"xMax":8,"yMin":-3,"yMax":8}'

quad_zh_1 = f"""<h2>一条曲线，三个旋钮</h2>
<p>二次函数 <latex>y = ax^2 + bx + c</latex> 看起来只有三项，却同时决定了三件事：开口朝哪边、曲线有多窄、以及它从哪里穿过坐标轴。</p>
<interactive type="canvas" title="拖动 a、b、c，看曲线怎么变" height="400"><![CDATA[{QUADRATIC_1}]]></interactive>
<p>先把 <b>a</b> 从 1 推到 −1。曲线翻了个身；再把 <b>c</b> 往下拉，整条曲线跟着下沉。每一个参数都不是符号，而是一个可以抓住的把手。</p>
<callout emoji="✳️" background-color="light-orange" border-color="orange"><p><b>观察：</b>当 <code>a &gt; 0</code> 时开口向上，函数有最小值；当 <code>a &lt; 0</code> 时开口向下，函数有最大值。顶点就是那个“最”的地方。</p></callout>
<h2>读图的三件事</h2>
<ul><li><b>开口方向</b>由 a 的符号决定。</li><li><b>对称轴</b>是 <latex>x = -b / (2a)</latex>。</li><li><b>与 y 轴的交点</b>永远是 <latex>(0, c)</latex>。</li></ul>
<blockquote><p>先看见，再命名。图不是公式的插图，公式是图的笔记。</p></blockquote>"""

quad_en_1 = f"""<h2>One curve, three knobs</h2>
<p>A quadratic <latex>y = ax^2 + bx + c</latex> has only three terms, yet they decide three things at once: which way the curve opens, how narrow it is, and where it crosses the axes.</p>
<interactive type="canvas" title="Drag a, b and c to reshape the curve" height="400"><![CDATA[{QUADRATIC_1}]]></interactive>
<p>Push <b>a</b> from 1 to −1 and the curve flips over. Pull <b>c</b> down and the whole curve follows. Each parameter is not a symbol; it is a handle you can hold.</p>
<callout emoji="✳️" background-color="light-orange" border-color="orange"><p><b>Notice:</b> when <code>a &gt; 0</code> the curve opens upward and has a minimum; when <code>a &lt; 0</code> it opens downward and has a maximum. The vertex is that “most” point.</p></callout>
<h2>Three things to read from the graph</h2>
<ul><li><b>Direction</b> is the sign of a.</li><li><b>The axis of symmetry</b> is <latex>x = -b / (2a)</latex>.</li><li><b>The y-intercept</b> is always <latex>(0, c)</latex>.</li></ul>
<blockquote><p>See it first, then name it. The graph is not an illustration of the formula; the formula is a note about the graph.</p></blockquote>"""

quad_zh_2 = f"""<h2>顶点从哪里来</h2>
<p>把 <latex>y = ax^2 + bx + c</latex> 配方，就能把顶点从公式里“请”出来：</p>
<p><latex>y = a\\left(x + \\frac{{b}}{{2a}}\\right)^2 + c - \\frac{{b^2}}{{4a}}</latex></p>
<p>所以顶点是 <latex>(-b/(2a),\\ c - b^2/(4a))</latex>。判别式 <latex>\\Delta = b^2 - 4ac</latex> 则告诉我们曲线与 x 轴有没有交点。</p>
<interactive type="canvas" title="a = −0.5 时，顶点落在哪里？" height="380"><![CDATA[{QUADRATIC_2}]]></interactive>
<table><thead><tr><th><p>判别式</p></th><th><p>实根个数</p></th><th><p>图像</p></th></tr></thead><tbody><tr><td><p><latex>\\Delta &gt; 0</latex></p></td><td><p>2</p></td><td><p>与 x 轴交两次</p></td></tr><tr><td><p><latex>\\Delta = 0</latex></p></td><td><p>1</p></td><td><p>顶点落在 x 轴上</p></td></tr><tr><td><p><latex>\\Delta &lt; 0</latex></p></td><td><p>0</p></td><td><p>与 x 轴不相交</p></td></tr></tbody></table>
<h2>轮到你</h2>
<checkbox done="false">把 a 调成 0，观察图像变成什么。它还是二次函数吗？</checkbox>
<checkbox done="false">找到一组 a、b、c，让顶点正好落在 (2, 3)。</checkbox>
<checkbox done="false">用判别式解释：为什么 a &lt; 0 且 c &lt; 0 时，曲线一定穿过 x 轴两次？</checkbox>"""

quad_en_2 = f"""<h2>Where the vertex comes from</h2>
<p>Complete the square on <latex>y = ax^2 + bx + c</latex> and the vertex falls out of the formula:</p>
<p><latex>y = a\\left(x + \\frac{{b}}{{2a}}\\right)^2 + c - \\frac{{b^2}}{{4a}}</latex></p>
<p>So the vertex is <latex>(-b/(2a),\\ c - b^2/(4a))</latex>. The discriminant <latex>\\Delta = b^2 - 4ac</latex> tells us whether the curve meets the x-axis at all.</p>
<interactive type="canvas" title="With a = −0.5, where is the vertex?" height="380"><![CDATA[{QUADRATIC_2}]]></interactive>
<table><thead><tr><th><p>Discriminant</p></th><th><p>Real roots</p></th><th><p>Graph</p></th></tr></thead><tbody><tr><td><p><latex>\\Delta &gt; 0</latex></p></td><td><p>2</p></td><td><p>Crosses the x-axis twice</p></td></tr><tr><td><p><latex>\\Delta = 0</latex></p></td><td><p>1</p></td><td><p>Vertex sits on the x-axis</p></td></tr><tr><td><p><latex>\\Delta &lt; 0</latex></p></td><td><p>0</p></td><td><p>Never meets the x-axis</p></td></tr></tbody></table>
<h2>Your turn</h2>
<checkbox done="false">Set a to 0 and watch the graph. Is it still a quadratic?</checkbox>
<checkbox done="false">Find a, b and c so the vertex lands exactly on (2, 3).</checkbox>
<checkbox done="false">Use the discriminant to explain why a &lt; 0 and c &lt; 0 always gives two x-intercepts.</checkbox>"""

PROJECTILE = '{"kind":"projectile","angle":45,"speed":14}'

physics_zh_1 = f"""<h2>把一秒钟拆成很多帧</h2>
<p>抛体运动最迷人的地方，是水平方向与竖直方向互不打扰。水平方向匀速，竖直方向只受重力。把这两个动作叠在一起，就得到一条抛物线。</p>
<interactive type="canvas" title="调整角度与初速度，然后发射" height="420"><![CDATA[{PROJECTILE}]]></interactive>
<p>固定初速度，把角度从 30° 调到 60°。射程先变长、再变短；45° 附近最远。最大高度却一直随角度上升。</p>
<callout emoji="🧪" background-color="light-blue" border-color="blue"><p><b>实验：</b>保持角度 45°，把初速度翻倍。射程会变成几倍？先猜，再按“发射”。</p></callout>
<table><thead><tr><th><p>量</p></th><th><p>公式</p></th><th><p>单位</p></th></tr></thead><tbody><tr><td><p>飞行时间</p></td><td><p><latex>T = 2v_0 \\sin\\theta / g</latex></p></td><td><p>s</p></td></tr><tr><td><p>最大高度</p></td><td><p><latex>H = v_0^2 \\sin^2\\theta / (2g)</latex></p></td><td><p>m</p></td></tr><tr><td><p>射程</p></td><td><p><latex>R = v_0^2 \\sin 2\\theta / g</latex></p></td><td><p>m</p></td></tr></tbody></table>"""

physics_en_1 = f"""<h2>Split one second into many frames</h2>
<p>The beautiful thing about projectile motion is that the horizontal and vertical stories ignore each other. Horizontally the speed is constant; vertically only gravity acts. Stack the two and you get a parabola.</p>
<interactive type="canvas" title="Set the angle and speed, then launch" height="420"><![CDATA[{PROJECTILE}]]></interactive>
<p>Keep the launch speed fixed and sweep the angle from 30° to 60°. The range first grows, then shrinks; it peaks near 45°. The maximum height, meanwhile, keeps climbing with the angle.</p>
<callout emoji="🧪" background-color="light-blue" border-color="blue"><p><b>Experiment:</b> at 45°, double the launch speed. What happens to the range? Guess first, then press Launch.</p></callout>
<table><thead><tr><th><p>Quantity</p></th><th><p>Formula</p></th><th><p>Unit</p></th></tr></thead><tbody><tr><td><p>Flight time</p></td><td><p><latex>T = 2v_0 \\sin\\theta / g</latex></p></td><td><p>s</p></td></tr><tr><td><p>Apex height</p></td><td><p><latex>H = v_0^2 \\sin^2\\theta / (2g)</latex></p></td><td><p>m</p></td></tr><tr><td><p>Range</p></td><td><p><latex>R = v_0^2 \\sin 2\\theta / g</latex></p></td><td><p>m</p></td></tr></tbody></table>"""

PENDULUM = """<!doctype html>
<html><body style="margin:0;display:grid;place-items:center;height:100%;background:#fbf8f1">
<canvas id="c" width="520" height="260" style="max-width:100%"></canvas>
<script>
const c = document.getElementById('c'), x = c.getContext('2d');
let a = 0.7, v = 0, last = performance.now();
function frame(t) {
  const dt = Math.min(0.032, (t - last) / 1000); last = t;
  v += (-9.8 / 0.9) * Math.sin(a) * dt; a += v * dt; v *= 0.999;
  x.clearRect(0, 0, c.width, c.height);
  const ox = c.width / 2, oy = 24, len = 190;
  const px = ox + Math.sin(a) * len, py = oy + Math.cos(a) * len;
  x.strokeStyle = '#d8d0c0'; x.lineWidth = 1;
  for (let i = 0; i < 5; i++) { x.beginPath(); x.arc(ox, oy, 40 + i * 36, 0, Math.PI * 2); x.stroke(); }
  x.strokeStyle = '#24211c'; x.lineWidth = 3; x.beginPath(); x.moveTo(ox, oy); x.lineTo(px, py); x.stroke();
  x.fillStyle = '#b45309'; x.beginPath(); x.arc(px, py, 15, 0, Math.PI * 2); x.fill();
  x.fillStyle = '#7a7264'; x.font = '12px system-ui'; x.fillText('点击画布，给单摆一个推力', 14, c.height - 14);
  requestAnimationFrame(frame);
}
c.addEventListener('click', () => { v += 1.4; });
requestAnimationFrame(frame);
</script></body></html>"""

physics_zh_2 = f"""<h2>沙箱里的小实验</h2>
<p>有些现象很难用几个参数描述。MarkdownX 允许把一段完整的 HTML/JS 放进独立沙箱：它不能读取这本书，也不能访问外部页面，但可以画布、动画、响应点击。</p>
<interactive type="html" title="单摆：点一下，给它一个推力" height="300"><![CDATA[{PENDULUM}]]></interactive>
<p>上面的单摆每一帧都在解 <latex>\\ddot{{\\theta}} = -(g/L)\\sin\\theta</latex>。它不是视频，而是读者可以打断、重新推动的模型。</p>
<callout emoji="🔒" background-color="light-gray" border-color="gray"><p><b>边界：</b>HTML 组件运行在 <code>sandbox="allow-scripts"</code> 的 iframe 里，无法读取宿主页面或本地文件。需要教材级稳定的交互时，优先用 Canvas 组件。</p></callout>"""

physics_en_2 = f"""<h2>A small experiment in a sandbox</h2>
<p>Some phenomena are hard to capture with a handful of parameters. MarkdownX can place a complete HTML/JS experiment in an isolated sandbox: it cannot read the book or reach the surrounding page, but it can draw, animate and respond to clicks.</p>
<interactive type="html" title="Pendulum: click to give it a push" height="300"><![CDATA[{PENDULUM}]]></interactive>
<p>The pendulum above solves <latex>\\ddot{{\\theta}} = -(g/L)\\sin\\theta</latex> every frame. It is not a video; it is a model the reader can interrupt and push again.</p>
<callout emoji="🔒" background-color="light-gray" border-color="gray"><p><b>Boundary:</b> HTML widgets run in an iframe with <code>sandbox="allow-scripts"</code> and cannot read the host page or local files. For stable textbook interactions, prefer the Canvas blocks.</p></callout>"""

physics_zh_3 = """<h2>记录一次实验</h2>
<p>下面是一段把发射数据记下来的小程序。它展示的是“书里可以放代码”，而不是让读者真的运行它。</p>
<pre lang="python" caption="记录射程与角度"><code>import math

def range_at(v0, angle_deg, g=9.8):
    angle = math.radians(angle_deg)
    return v0 * v0 * math.sin(2 * angle) / g

for angle in (15, 30, 45, 60, 75):
    print(angle, round(range_at(14, angle), 2))</code></pre>
<p>把输出抄进下面的表，再和画布上的读数对一遍。数据与图像互相校验，结论才站得住。</p>
<table><thead><tr><th><p>角度</p></th><th><p>射程 / m</p></th><th><p>你的预测</p></th></tr></thead><tbody><tr><td><p>30°</p></td><td><p></p></td><td><p></p></td></tr><tr><td><p>45°</p></td><td><p></p></td><td><p></p></td></tr><tr><td><p>60°</p></td><td><p></p></td><td><p></p></td></tr></tbody></table>"""

physics_en_3 = """<h2>Record one experiment</h2>
<p>Here is a tiny program that tabulates launch data. It shows that a book can hold code, not that the reader has to run it.</p>
<pre lang="python" caption="Record range by angle"><code>import math

def range_at(v0, angle_deg, g=9.8):
    angle = math.radians(angle_deg)
    return v0 * v0 * math.sin(2 * angle) / g

for angle in (15, 30, 45, 60, 75):
    print(angle, round(range_at(14, angle), 2))</code></pre>
<p>Copy the output into the table below and compare it with the readings on the canvas. Data and picture check each other; that is what makes a conclusion hold.</p>
<table><thead><tr><th><p>Angle</p></th><th><p>Range / m</p></th><th><p>Your prediction</p></th></tr></thead><tbody><tr><td><p>30°</p></td><td><p></p></td><td><p></p></td></tr><tr><td><p>45°</p></td><td><p></p></td><td><p></p></td></tr><tr><td><p>60°</p></td><td><p></p></td><td><p></p></td></tr></tbody></table>"""

type_md = """## 一本书的两种呼吸

Markdown 适合**写得快**的部分。它像草稿纸：标题、列表、引用、代码，抬手就有。

> 好的排版不是把页面填满，而是让读者知道下一步该看哪里。

### 一份最小的样式表

| 元素 | 作用 | 读者感知 |
| --- | --- | --- |
| 衬线正文 | 长文阅读 | 慢下来 |
| 无衬线标签 | 导航与注释 | 快速定位 |
| 等宽代码 | 结构与参数 | 精确 |

```js
const book = {
  title: "会动的教科书",
  languages: ["zh", "en"],
  interactive: true,
};
```

```mdxb-canvas
{"title":"把参数写进 Markdown","height":320,"source":"{\\"kind\\":\\"quadratic\\",\\"a\\":1,\\"b\\":0,\\"c\\":0,\\"xMin\\":-4,\\"xMax\\":4,\\"yMin\\":-4,\\"yMax\\":8}"}
```

最后一行代码块不是普通代码，而是一个 Canvas 组件的 JSON 外壳。Markdown 也能长出可交互的部分。"""

type_md_en = """## Two ways for a book to breathe

Markdown is for the parts you want to **write quickly**. It is scratch paper: headings, lists, quotes and code are one keystroke away.

> Good typography is not about filling the page. It is about telling the reader where to look next.

### A minimal type table

| Element | Job | How it feels |
| --- | --- | --- |
| Serif body | Long-form reading | Slow down |
| Sans labels | Navigation and notes | Locate fast |
| Mono code | Structure and parameters | Be exact |

```js
const book = {
  title: "A textbook that moves",
  languages: ["zh", "en"],
  interactive: true,
};
```

```mdxb-canvas
{"title":"Parameters inside Markdown","height":320,"source":"{\\"kind\\":\\"quadratic\\",\\"a\\":1,\\"b\\":0,\\"c\\":0,\\"xMin\\":-4,\\"xMax\\":4,\\"yMin\\":-4,\\"yMax\\":8}"}
```

That last fence is not ordinary code; it is the JSON shell of a Canvas block. Markdown can grow interactive parts too."""

type_xml = """<h2>同一页，三种语气</h2>
<p>标题是断言，正文是解释，注释是旁白。把它们放在同一页，读者才知道哪里该快、哪里该慢。</p>
<grid><column width-ratio="0.55"><h3>正文</h3><p>用衬线字体承载长句。行高 <code>1.82</code>，每行大约 34 个汉字，视线不必来回折返。</p></column><column width-ratio="0.45"><h3>旁注</h3><p>用无衬线字体承载标签、图注与控件。它不抢正文的注意力，但随时可以被找到。</p></column></grid>
<callout emoji="Aa" background-color="light-purple" border-color="purple"><p><b>双语不是翻译：</b>中文段落更短、更依赖语境；英文段落更依赖连接词。两种版本应该分别写，而不是逐句替换。</p></callout>
<table><thead><tr><th><p>层级</p></th><th><p>字体</p></th><th><p>用途</p></th></tr></thead><tbody><tr><td><p>H1</p></td><td><p>衬线 54px</p></td><td><p>章标题</p></td></tr><tr><td><p>H2</p></td><td><p>衬线 30px</p></td><td><p>节标题</p></td></tr><tr><td><p>Body</p></td><td><p>衬线 18px</p></td><td><p>正文</p></td></tr><tr><td><p>UI</p></td><td><p>无衬线 12px</p></td><td><p>标签与控件</p></td></tr></tbody></table>
<checkbox done="true">读一遍中文，再读一遍英文。</checkbox>
<checkbox done="false">把一段英文改短，看看意思有没有丢。</checkbox>"""

type_xml_en = """<h2>One page, three voices</h2>
<p>A heading asserts. Body text explains. A note stands beside the page. Put all three together and the reader knows where to speed up and where to linger.</p>
<grid><column width-ratio="0.55"><h3>Body</h3><p>Set long sentences in a serif. Line height <code>1.82</code>, roughly 34 Chinese characters per line, so the eye does not shuttle back and forth.</p></column><column width-ratio="0.45"><h3>Margin</h3><p>Set labels, captions and controls in a sans. They should not compete with the body, but they must stay findable.</p></column></grid>
<callout emoji="Aa" background-color="light-purple" border-color="purple"><p><b>Bilingual is not translation:</b> Chinese paragraphs lean shorter and more on context; English paragraphs lean on connectives. Write each version on its own terms, not sentence by sentence.</p></callout>
<table><thead><tr><th><p>Level</p></th><th><p>Face</p></th><th><p>Job</p></th></tr></thead><tbody><tr><td><p>H1</p></td><td><p>Serif 54px</p></td><td><p>Chapter</p></td></tr><tr><td><p>H2</p></td><td><p>Serif 30px</p></td><td><p>Section</p></td></tr><tr><td><p>Body</p></td><td><p>Serif 18px</p></td><td><p>Reading</p></td></tr><tr><td><p>UI</p></td><td><p>Sans 12px</p></td><td><p>Labels and controls</p></td></tr></tbody></table>
<checkbox done="true">Read the Chinese once, then the English.</checkbox>
<checkbox done="false">Shorten one English paragraph and see what survives.</checkbox>"""


def main() -> None:
    quadratics = book(
        "quadratics",
        ("二次函数：把抛物线握在手里", "Quadratics in Your Hands"),
        ("从一条曲线开始，读懂三个参数。", "Start with one curve and learn to read three parameters."),
        "#b45309",
        [
            chapter("curve", ("从一条曲线开始", "Start with a Curve"), quad_zh_1, quad_en_1),
            chapter("vertex", ("顶点与判别式", "Vertex and Discriminant"), quad_zh_2, quad_en_2),
        ],
        ["MarkdownX 教材组"],
    )
    physics = book(
        "projectile-lab",
        ("物理实验室：抛物线", "Physics Lab: Projectile"),
        ("调参数、发射、记录，再把结论写下来。", "Tune, launch, record, then write the conclusion."),
        "#2f6b5f",
        [
            chapter("launch", ("抛体运动", "Projectile Motion"), physics_zh_1, physics_en_1),
            chapter("sandbox", ("HTML 沙箱", "The HTML Sandbox"), physics_zh_2, physics_en_2),
            chapter("record", ("实验记录", "Lab Notes"), physics_zh_3, physics_en_3),
        ],
        ["MarkdownX 物理组"],
    )
    specimen = book(
        "type-specimen",
        ("双语排版样本", "A Bilingual Type Specimen"),
        ("同一本书，两种语言，各自呼吸。", "One book, two languages, each with its own breath."),
        "#5b4b8a",
        [
            chapter("markdown", ("Markdown 的一页", "A Markdown page"), type_md, type_md_en, fmt="markdown"),
            chapter("docxxml", ("DocxXML 的一页", "A DocxXML page"), type_xml, type_xml_en),
        ],
        ["MarkdownX 设计组"],
    )
    for data, name in ((quadratics, "quadratics.mdxb"), (physics, "projectile-lab.mdxb"), (specimen, "type-specimen.mdxb")):
        (ROOT / name).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    manifest = {
        "books": [
            {
                "file": "quadratics.mdxb",
                "kicker": "Mathematics · 数学",
                "cover": {"bg": "#8a3f06", "ink": "#fff7ec", "glyph": "ƒ"},
                "zh": {"title": "二次函数", "blurb": "拖动 a、b、c，看顶点与判别式如何改变一条抛物线。", "tags": ["数学", "Canvas", "双语"]},
                "en": {"title": "Quadratics", "blurb": "Drag a, b and c and watch the vertex and discriminant reshape a parabola.", "tags": ["Math", "Canvas", "Bilingual"]},
            },
            {
                "file": "projectile-lab.mdxb",
                "kicker": "Physics · 物理",
                "cover": {"bg": "#23473f", "ink": "#eef2e8", "glyph": "→"},
                "zh": {"title": "抛体实验室", "blurb": "调整角度与初速度，发射，然后把数据抄进表格。", "tags": ["物理", "Canvas", "HTML 沙箱"]},
                "en": {"title": "Projectile Lab", "blurb": "Tune the angle and speed, launch, then copy the data into the table.", "tags": ["Physics", "Canvas", "HTML sandbox"]},
            },
            {
                "file": "type-specimen.mdxb",
                "kicker": "Typography · 排版",
                "cover": {"bg": "#3e3457", "ink": "#f1ecff", "glyph": "Aa"},
                "zh": {"title": "双语排版样本", "blurb": "Markdown 与 DocxXML 同页，中文与英文各自成篇。", "tags": ["排版", "Markdown", "DocxXML"]},
                "en": {"title": "Bilingual Specimen", "blurb": "Markdown and DocxXML side by side, Chinese and English each on their own terms.", "tags": ["Type", "Markdown", "DocxXML"]},
            },
        ]
    }
    (ROOT / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print("built", ", ".join(p.name for p in sorted(ROOT.glob("*.mdxb"))))


if __name__ == "__main__":
    main()
