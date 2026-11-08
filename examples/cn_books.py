#!/usr/bin/env python3
"""Two Chinese middle-school demo books.

Kept separate from ``build.py`` so the English type specimen and the maths
demo stay easy to read. ``build.py`` imports ``books()`` and appends them to
the reader's manifest.
"""
from __future__ import annotations

import json
import time
from pathlib import Path

NOW = int(time.time() * 1000)


def loc(zh: str, en: str) -> dict[str, str]:
    return {"zh": zh, "en": en}


def chapter(cid: str, title: tuple[str, str], content: str, content_en: str = "") -> dict:
    return {
        "id": cid,
        "title": loc(*title),
        "format": "markdown",
        "content": content,
        "content_en": content_en or content,
    }


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
        "generator": "MarkdownX examples/cn_books.py",
    }


def canvas(title: str, spec: dict, height: int = 380) -> str:
    payload = {"title": title, "height": height, "source": json.dumps(spec, ensure_ascii=False)}
    return "```mdxb-canvas\n" + json.dumps(payload, ensure_ascii=False) + "\n```"


def html_widget(title: str, source: str, height: int = 380) -> str:
    payload = {"title": title, "height": height, "source": source}
    return "```mdxb-html\n" + json.dumps(payload, ensure_ascii=False) + "\n```"


# ---------------------------------------------------------------- physics

PHYSICS_1 = f"""
一辆小车从站台出发。说“它在动”，总要先说“相对于谁”——这就是参照物。

| 参照物 | 小车的运动 |
| --- | --- |
| 站台 | 向前运动 |
| 小车上的乘客 | 静止 |
| 迎面驶来的车 | 更快地靠近 |

速度等于路程除以时间：

$v = \\frac{{s}}{{t}}$

{canvas("拖动 a、b，改变路程图像", {"kind": "quadratic", "a": 0.0, "b": 2.0, "c": 0.0, "xMin": 0, "xMax": 8, "yMin": 0, "yMax": 12})}

把 **a** 留在 0，图像是一条直线：路程随时间均匀增加，这就是匀速直线运动。把 **a** 调大，图像向上弯，小车在加速。
"""

PHYSICS_1_EN = """
A cart leaves the platform. To say it is moving, you must first say “relative to what” — that is the reference object.

| Reference | The cart |
| --- | --- |
| The platform | Moving forward |
| A passenger | At rest |
| An oncoming cart | Approaching faster |

Speed is distance divided by time:

$v = \\frac{{s}}{{t}}$

Drag **a** and **b** to change the distance–time graph. With **a** at 0 the graph is a straight line: constant speed. Increase **a** and it curves upward: the cart is accelerating.
"""

PHYSICS_2 = """
### 力不是“推一下”就没了

力有三要素：大小、方向、作用点。它们共同决定力的效果。

| 力 | 方向 | 常见例子 |
| --- | --- | --- |
| 重力 | 竖直向下 | 苹果落地 |
| 支持力 | 垂直于接触面 | 桌面对书 |
| 摩擦力 | 与相对运动方向相反 | 刹车 |

当物体在两个力作用下保持静止或匀速直线运动，这两个力**平衡**：大小相等、方向相反、作用在同一直线上。

> 实验：用弹簧测力计水平拉木块，读出拉力；换不同粗糙程度的桌面，比较滑动摩擦力。
"""

PHYSICS_2_EN = """
A force has size, direction and point of application. Together they decide its effect.

| Force | Direction | Example |
| --- | --- | --- |
| Gravity | Straight down | An apple falls |
| Normal force | Perpendicular to the surface | A table holds a book |
| Friction | Opposite to relative motion | Braking |

When two forces leave an object at rest or moving at constant speed, they are **balanced**: equal size, opposite direction, same line of action.

> Lab: pull a block with a spring scale, then repeat on rougher surfaces and compare friction.
"""

PHYSICS_3 = """
### 记录，而不是“感觉”

| 次数 | 路程 s / m | 时间 t / s | 速度 v / (m·s⁻¹) |
| --- | --- | --- | --- |
| 1 | 1.00 | 2.0 | 0.50 |
| 2 | 2.00 | 4.0 | 0.50 |
| 3 | 3.00 | 6.0 | 0.50 |

- [x] 检查单位
- [x] 保留两位有效数字
- [ ] 换一辆小车，重做一次

结论：在误差允许的范围内，速度不随路程改变。
"""

PHYSICS_3_EN = """
Measure, do not guess.

| Run | Distance s / m | Time t / s | Speed v / (m·s⁻¹) |
| --- | --- | --- | --- |
| 1 | 1.00 | 2.0 | 0.50 |
| 2 | 2.00 | 4.0 | 0.50 |
| 3 | 3.00 | 6.0 | 0.50 |

- [x] Check the units
- [x] Keep two significant figures
- [ ] Repeat with another cart

Conclusion: within experimental error, speed does not change with distance.
"""

# ---------------------------------------------------------------- chemistry

CHEM_1 = """
化学从“观察”开始，而不是从“背结论”开始。

| 仪器 | 用途 | 注意 |
| --- | --- | --- |
| 试管 | 少量反应 | 加热时管口不对人 |
| 量筒 | 量取液体 | 读数视线与凹液面最低处齐平 |
| 酒精灯 | 加热 | 用外焰，不用嘴吹灭 |

> 实验台上的规则不是装饰：闻气体要用手轻轻扇动，不能把鼻子凑到瓶口。
"""

CHEM_1_EN = """
Chemistry starts with observation, not memorised conclusions.

| Apparatus | Use | Care |
| --- | --- | --- |
| Test tube | Small reactions | Point away from people when heating |
| Measuring cylinder | Measure liquid | Read at the bottom of the meniscus |
| Alcohol lamp | Heat | Use the outer flame; never blow it out |

> Lab rules are not decoration: waft a gas toward your nose, never put your nose over the mouth of a bottle.
"""

CHEM_2 = f"""
物质由分子、原子等微观粒子构成。一滴水里有大约 $1.7 \\times 10^{{21}}$ 个水分子。

| 粒子 | 特征 |
| --- | --- |
| 分子 | 保持物质化学性质的最小粒子 |
| 原子 | 化学变化中的最小粒子 |
| 离子 | 带电的原子或原子团 |

{html_widget("把一滴水放大", '''<!doctype html><meta charset="utf-8"><style>
html,body{margin:0;height:100%;background:#f7f2e8;font-family:system-ui;overflow:hidden}
canvas{display:block;width:100%;height:100%}
p{position:absolute;left:16px;bottom:12px;margin:0;color:#6c5c49;font-size:12px}
</style><canvas></canvas><p>点击画布，给粒子一点扰动</p><script>
const c=document.querySelector('canvas'),x=c.getContext('2d');let d=[],w,h;
function fit(){w=c.width=c.clientWidth*devicePixelRatio;h=c.height=c.clientHeight*devicePixelRatio;x.setTransform(devicePixelRatio,0,0,devicePixelRatio,0,0);}
function seed(){d=[];for(let i=0;i<70;i++)d.push({x:Math.random()*innerWidth,y:Math.random()*innerHeight,vx:(Math.random()-.5)*.5,vy:(Math.random()-.5)*.5,r:2+Math.random()*3});}
addEventListener('resize',()=>{fit();seed();});fit();seed();requestAnimationFrame(()=>{fit();seed();});
c.onclick=()=>d.forEach(p=>{p.vx+=(Math.random()-.5)*2;p.vy+=(Math.random()-.5)*2;});
(function loop(){if(!c.width)fit();x.clearRect(0,0,innerWidth,innerHeight);for(const p of d){p.x+=p.vx;p.y+=p.vy;if(p.x<0||p.x>innerWidth)p.vx*=-1;if(p.y<0||p.y>innerHeight)p.vy*=-1;x.beginPath();x.arc(p.x,p.y,p.r,0,7);x.fillStyle='#b05226';x.fill();}requestAnimationFrame(loop);})();
</script>''')}

粒子在不停运动，温度越高，运动越剧烈。这就是“扩散”的微观解释。
"""

CHEM_2_EN = """
Matter is made of molecules, atoms and ions. One drop of water holds about $1.7 × 10^21$ water molecules.

| Particle | What it is |
| --- | --- |
| Molecule | Smallest particle that keeps the chemical properties |
| Atom | Smallest particle in a chemical change |
| Ion | A charged atom or group of atoms |

Click the canvas to disturb the particles. They move constantly; the hotter the substance, the faster they move. That is diffusion at the particle level.
"""

CHEM_3 = """
化学方程式用符号记录反应，两边每一种原子的数目必须相等。

| 反应 | 方程式 |
| --- | --- |
| 木炭燃烧 | $C + O_2 \\rightarrow CO_2$ |
| 氢气燃烧 | $2H_2 + O_2 \\rightarrow 2H_2O$ |
| 实验室制氧气 | $2KMnO_4 \\rightarrow K_2MnO_4 + MnO_2 + O_2\\uparrow$ |

配平步骤：

1. 写出反应物和生成物的化学式；
2. 配平原子个数，先配复杂物质；
3. 检查条件、气体和沉淀符号；
4. 把箭头改成等号。

- [x] 两边原子数相等
- [x] 化学式没有改错
- [ ] 标出反应条件
"""

CHEM_3_EN = """
A chemical equation records a reaction with symbols. The number of each kind of atom must match on both sides.

| Reaction | Equation |
| --- | --- |
| Burning charcoal | $C + O_2 \\rightarrow CO_2$ |
| Burning hydrogen | $2H_2 + O_2 \\rightarrow 2H_2O$ |
| Making oxygen | $2KMnO_4 \\rightarrow K_2MnO_4 + MnO_2 + O_2\\uparrow$ |

Balance in four steps: write the formulas, balance atoms starting with the most complex substance, add conditions and state symbols, then replace the arrow with an equals sign.

- [x] Atoms balance
- [x] Formulas are unchanged
- [ ] Conditions marked
"""


def books() -> list[tuple[dict, str]]:
    physics = book(
        "junior-physics",
        ("初中物理：力与运动", "Middle School Physics: Force and Motion"),
        ("从一辆小车开始，学会读图、测量和记录。", "Start with a cart and learn to read graphs, measure and record."),
        "#1f5f8b",
        [
            chapter("motion", ("机械运动", "Motion"), PHYSICS_1, PHYSICS_1_EN),
            chapter("forces", ("力与平衡", "Forces in Balance"), PHYSICS_2, PHYSICS_2_EN),
            chapter("record", ("实验记录", "Lab Notes"), PHYSICS_3, PHYSICS_3_EN),
        ],
        ["MarkdownX 初中物理组"],
    )
    chemistry = book(
        "junior-chemistry",
        ("初中化学：物质与变化", "Middle School Chemistry: Matter and Change"),
        ("从一滴水到一条方程式，看见看不见的粒子。", "From a drop of water to an equation, see the particles you cannot see."),
        "#7a4b8f",
        [
            chapter("lab", ("走进化学实验室", "Inside the Lab"), CHEM_1, CHEM_1_EN),
            chapter("particles", ("分子、原子与元素", "Particles and Elements"), CHEM_2, CHEM_2_EN),
            chapter("equations", ("化学方程式", "Chemical Equations"), CHEM_3, CHEM_3_EN),
        ],
        ["MarkdownX 初中化学组"],
    )
    return [(physics, "junior-physics.mdxb"), (chemistry, "junior-chemistry.mdxb")]


def manifest_entries() -> list[dict]:
    return [
        {
            "file": "junior-physics.mdxb",
            "kicker": "Physics · 初中物理",
            "cover": {"bg": "#1f4e6b", "ink": "#eef7fb", "glyph": "F"},
            "zh": {"title": "力与运动", "blurb": "参照物、速度、二力平衡，以及一张会动的路程图。", "tags": ["物理", "表格", "Canvas"]},
            "en": {"title": "Force and Motion", "blurb": "Reference frames, speed, balanced forces, and a distance graph you can drag.", "tags": ["Physics", "Tables", "Canvas"]},
        },
        {
            "file": "junior-chemistry.mdxb",
            "kicker": "Chemistry · 初中化学",
            "cover": {"bg": "#52345f", "ink": "#fbf3ff", "glyph": "C"},
            "zh": {"title": "物质与变化", "blurb": "从实验台到粒子，再到配平的化学方程式。", "tags": ["化学", "表格", "HTML 沙箱"]},
            "en": {"title": "Matter and Change", "blurb": "From the lab bench to particles, then to balanced equations.", "tags": ["Chemistry", "Tables", "HTML sandbox"]},
        },
    ]
