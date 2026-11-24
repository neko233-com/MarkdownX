# codegraph233 MCP

MarkdownX uses [codegraph233](https://github.com/neko233-com/codegraph233) as a
local, read-only code graph. It answers "where is this symbol declared", "who
calls it", and "what does this change touch" without grepping the whole tree.

## Install

codegraph233 is a single Rust binary served over MCP stdio. Install it once
from its own repository (not from MarkdownX):

```sh
git clone https://github.com/neko233-com/codegraph233
cd codegraph233

# Windows
powershell -ExecutionPolicy Bypass -File install.ps1

# macOS / Linux
./install.sh
```

`codegraph233 version` should print a version; MarkdownX is developed against
`0.4.1`. The project already commits:

- `.codegraph233.json` — Go, TypeScript, JavaScript, Python and PowerShell,
  with generated output excluded.
- `opencode.json` — starts `codegraph233 serve --root .` for this workspace.

The index itself lives in `.codegraph233/` and is ignored by Git. It is local
state: delete it and rebuild at any time.

## Build the index

```sh
codegraph233 index --root .
codegraph233 status --root .
```

The first run is a full index; later runs are incremental. Use `--force` after
a large rewrite or a parser/config change.

## Tools (OpenCode Code Mode)

Tools are grouped under `tools.codegraph233`:

| Tool | Use it for |
| --- | --- |
| `cg233_status` | Index freshness, counts, schema |
| `cg233_overview` | Languages, namespaces, symbol kinds, health |
| `cg233_file_outline` | Declarations in one file |
| `cg233_find_symbol` | Search by name or qualified name |
| `cg233_get_symbol` | Read one symbol, optionally with source |
| `cg233_find_referencing_symbols` | Who calls or references it |
| `cg233_call_graph` | Callers/callees, up to N hops |
| `cg233_impact` | Change-impact set for a symbol or file |
| `cg233_search_pattern` | Regex search with context |
| `cg233_symbol_path` | Namespace → owning type → symbol chain |
| `cg233_project_health` | Unresolved refs, DB size, coverage |

Typical order for a change:

1. `cg233_file_outline` on the files you expect to touch.
2. `cg233_find_symbol` for the API you need.
3. `cg233_find_referencing_symbols` / `cg233_impact` before editing.
4. `cg233_search_pattern` for text that is not a symbol (messages, CSS, JSON).

## Notes

- The server never edits files. It is safe to leave running while editing.
- After a large refactor, run `codegraph233 index --root .` again.
- If OpenCode was already running when `opencode.json` was added, run
  `/reload` or restart the session so the MCP server is picked up.
- On a machine without the binary, the `codegraph233` server stays
  disconnected; nothing else in the build or tests depends on it.
