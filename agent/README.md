# Agent-authored books

MarkdownX is designed so an agent can write a book as a folder of Markdown
chapters, validate it without opening the GUI, and hand the same folder to a
person for editing. The `.mdxb` file is the distribution artifact; the folder
is the source.

```text
books/physics/
  book.json                 # optional metadata preserved by unpack
  01-motion.md              # one chapter per file, sorted by name
  02-forces.md
  03-lab.md
  assets/                   # local images referenced from the chapters
```

`book.json` is written by `unpack`; it keeps titles, ids and `content_en`.
When present, `pack` reuses those chapter titles instead of filenames.

The desktop `下载为 → Markdown 文件夹 (.md)` writes the same kind of folder,
plus a same-stem `.xml` DocxXML source beside each chapter. That XML is the
lossless fallback for callouts, grids and other editor-only structure. On
repack, the XML is used only while it is at least as new as the `.md`; once you
edit a chapter's Markdown, the Markdown wins. `pack` accepts `.md` and `.xml`
chapter sources, reads `NN-topic.en.md` as `content_en`, and re-embeds files
under `assets/`.

## Commands

No dependencies beyond Python 3.10+.

```sh
# Validate one book or every .mdxb under a folder.
python tools/agent/mdxb.py check examples

# Folder of Markdown -> one book.
python tools/agent/mdxb.py pack books/physics \
  --title "初中物理：力与运动" --title-en "Force and Motion" \
  --author "MarkdownX 物理组" -o physics.mdxb

# Book -> editable folder (book.json, chapters, assets).
python tools/agent/mdxb.py unpack physics.mdxb books/physics

# Fuzzy path search; --content also reads inside the chapters.
python tools/agent/mdxb.py search books/physics 速度 --content

# Merge annotation sidecars from two reviewers, newest edit wins.
python tools/agent/mdxb.py merge-annotations merged.json \
  alice.md.annotations.json bob.md.annotations.json
```

`check` exits 1 on any error and prints every problem it finds. It checks the
format/version, duplicate and empty chapters, DocxXML well-formedness, safe
asset keys, base64 payloads and the 16 MiB / 64 MiB limits.

## Chapter dialect

Chapters are ordinary Markdown plus a few fenced blocks:

| Need | Write |
| --- | --- |
| Heading, list, quote, table | normal Markdown |
| Inline maths | `$v = \frac{s}{t}$` |
| Native canvas | ` ```mdxb-canvas ` with a JSON body |
| Sandboxed HTML | ` ```mdxb-html ` with a JSON body |
| Local image | `![](assets/diagram.png)` |
| Checklist | `- [x] measured` / `- [ ] repeat` |

Canvas JSON:

````markdown
```mdxb-canvas
{"title":"拖动参数，改变路程图像","height":380,
 "source":"{\"kind\":\"quadratic\",\"a\":0,\"b\":2,\"c\":0,\"xMin\":0,\"xMax\":8,\"yMin\":0,\"yMax\":12}"}
```
````

`source` is a JSON **string**. Web supports `quadratic`, `unitcircle` and
`projectile`; the desktop native runtime currently implements `quadratic`.
HTML is executed only by the web reader, inside
`<iframe sandbox="allow-scripts">`.

## Annotations are a sidecar

A Markdown file `lesson.md` is reviewed in `lesson.md.annotations.json`; a
library page uses `<library>/annotations/<page-id>.json`. The document itself
is never rewritten to add comments.

```json
{
  "format": "markdownx.annotations",
  "version": 1,
  "doc": "lesson.md",
  "annotations": [
    {
      "id": "a1",
      "author": {"id": "alice", "name": "Alice", "color": "#b05226"},
      "body": "这里需要先说明参照物。",
      "quote": "一辆小车从站台出发。",
      "anchor": [2, 0],
      "created": 1760000000000,
      "updated": 1760000000000,
      "resolved": false,
      "replies": []
    }
  ]
}
```

`anchor` is a block path from the document root; `quote` is a snapshot for
humans when the block moves. Agents should add anchors rather than editing the
source to insert comment markers.

## The loop

1. Write or edit `NN-topic.md`; keep one H1 at the top if you want the title
   derived automatically.
2. Run `pack` and `check`.
3. Open the generated book in the web reader (`python -m http.server 8765`,
   then `/web/`) or in the desktop app.
4. Read annotations, apply accepted edits to the Markdown source, and merge
   sidecars with `merge-annotations` when several reviewers worked offline.
5. Commit the source folder and the generated `.mdxb` together when the book
   is a release artifact.

See `docs/FORMAT.md` for the byte-level `.mdxb` contract and `agent/AGENTS.md`
for rules to give a coding agent.
