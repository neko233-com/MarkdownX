# Rules for agents editing books

You are working on a MarkdownX book, not a web page.

- The source of truth is the folder of `NN-topic.md` chapters. `.mdxb` is a
  build artifact. Edit the Markdown, then run `pack`.
- Read `docs/FORMAT.md` before changing `book.json`, chapter JSON or assets.
  Do not invent new top-level keys; unknown keys are dropped by `Book::parse`.
- Keep one `# Chapter title` at the top of each chapter unless `book.json`
  already supplies the title. `pack` strips it into the chapter title.
- Prefer Markdown tables, `$...$` maths and fenced `mdxb-canvas` blocks over
  screenshots. Canvas `source` is a JSON string, not a nested object.
- HTML is a sandboxed escape hatch. It may not read the host page; never put
  secrets or network calls in it. Prefer Canvas for textbook interactions.
- Images go under `assets/` and are referenced as `assets/name.png`. Never
  write absolute paths, `..`, backslashes or `:` into an asset key.
- Annotations belong in `.annotations.json` sidecars, keyed by block path.
  Do not insert comment markers into the prose.
- Run `python tools/agent/mdxb.py check <book-or-folder>` after every pack.
  A release is not ready while `check` fails.
- Run `python -m py_compile` after changing `tools/agent/mdxb.py` or
  `examples/build.py`.
- Do not hand-edit generated `.mdxb` JSON if the chapter source exists; fix
  the source and repack.
