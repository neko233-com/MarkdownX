# MarkdownX server

A small self-hosted server for a team's Markdown workspace:

- the Git repository root **is** the workspace; every `.md` file is a document;
- nested folders, Chinese filenames and spaces are supported;
- every accepted save is a Git commit;
- browser editor with live presence and conflict handling;
- workflow templates for game design, product, film and content teams;
- history, unified diff and restore over HTTP;
- one Rust binary, no database.

It is intentionally small. It is not a CRDT service and it is not a multi-tenant
SaaS; read [Security](#security) before exposing it.

## Quick start

Requires Rust 1.99+ and `git` on `PATH`.

```sh
cargo run -p markdownx-server -- \
  -root ./data -web server/web
```

Open <http://127.0.0.1:8877/>. If no token is configured and `-dev` is not set,
the server generates a token and prints it to the log; paste it into the token
dialog.

For local development, `-dev` disables the token requirement and allows any
browser origin:

```sh
cargo run -p markdownx-server -- -root ./.data -web server/web -dev
```

## Configuration

Flags override environment variables; both override defaults.

| Flag | Environment | Default | Meaning |
| --- | --- | --- | --- |
| `-addr` | `MARKDOWNX_SERVER_ADDR` | `:8877` | Listen address. |
| `-root` | `MARKDOWNX_SERVER_ROOT` | `./data` | Workspace root; created if missing. |
| `-token` | `MARKDOWNX_SERVER_TOKEN` | empty | Shared bearer token. |
| `-origin` | `MARKDOWNX_SERVER_ORIGIN` | empty | Exact allowed browser origin. |
| `-max-bytes` | — | `2097152` | Maximum document size (1 KiB–64 MiB). |
| `-git-name` | `MARKDOWNX_GIT_NAME` | `MarkdownX` | Git author name. |
| `-git-email` | `MARKDOWNX_GIT_EMAIL` | `server@markdownx.local` | Git author email. |
| `-web` | `MARKDOWNX_SERVER_WEB` | `web` | Static editor directory. |
| `-dev` | — | `false` | Permissive origin, tokenless access, request logs. |

`-origin` is checked for WebSocket upgrades. Leave it empty when the editor is
served by this server on the same host; set it to the public origin when the
editor is served separately.

## Data layout

```text
<root>/                     # the .git directory of the team repository
├── .git/                   # one commit per accepted save
├── design/game/combat.md
├── product/prd.md
├── film/treatment.md
└── docs/                   # default folder for new documents
```

Document IDs are repository-relative paths (`design/game/combat.md`), encoded
with `encodeURIComponent` in URLs. Segments cannot be `.`/`..`, start with a
dot, or contain control characters and Windows-reserved characters. The title
is the first `# ` heading, falling back to the file stem. Saves use a temporary
file, `fsync`, then `rename`.

The Git repository is initialized inside `<root>` only when `<root>/.git` does
not already exist. If `<root>` is inside a larger repository, the server does
not commit into the parent repository.

## HTTP API

All routes except `/api/health` require the token:

```http
Authorization: Bearer <token>
```

The editor can also pass `?token=<token>`; the browser stores it locally and
uses it for both HTTP and WebSocket requests.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/health` | Liveness; no auth. |
| `GET` | `/api/templates` | Workflow starter list. |
| `GET` | `/api/docs` | List every Markdown file in the workspace. |
| `POST` | `/api/docs` | Create `{title, content?, folder?, template?, id?}`. |
| `GET` | `/api/doc?id=<path>` | Read `{id, title, content, doc}`. |
| `PUT` | `/api/doc?id=<path>` | Save `{base, content}`; `409` on conflict. |
| `GET` | `/api/history?id=<path>&limit=50` | Git commits for the file. |
| `GET` | `/api/diff?id=<path>&from=&to=` | Unified diff for a commit range. |
| `GET` | `/api/revision?id=<path>&rev=<hash>` | File content at a Git revision. |
| `POST` | `/api/restore?id=<path>` | Restore `{rev: "<git-hash>"}` as a new commit. |
| `GET` | `/ws/doc?id=<path>` | WebSocket session. |

`folder` is a repository-relative folder; new documents default to `docs/`.
`template` is one of `game-design`, `level-design`, `narrative`,
`product-prd`, `film-treatment`, `content-article`, `meeting-notes`.

`base` is the room revision returned by the last `snapshot` or `saved`
message. On a stale base the server attempts a three-way merge with
`git merge-file`; the response is still `409` if the merge conflicts. Treat
`409` as "reload and reapply", not as a retry of the same bytes.

## WebSocket protocol

Connect to `/ws/doc?id=<path>&name=<display-name>&token=<token>`.

Server to client messages:

```json
{"type":"snapshot","client_id":"...","id":"lesson","title":"Lesson","rev":3,"content":"# Lesson\n","peers":[]}
{"type":"update","rev":4,"title":"Lesson","by":"Reader-a1","origin":"...","at":1760000000000,"content":"# Lesson\n..."}
{"type":"saved","rev":4,"by":"Reader-a1","at":1760000000000,"commit":"<sha>","warning":""}
{"type":"conflict","rev":4,"content":"...","merged":"...","by":"Reader-b2"}
{"type":"presence","peers":[{"id":"...","name":"Reader-a1","color":"#..."}]}
{"type":"error","message":"..."}
{"type":"pong"}
```

Client to server messages:

```json
{"type":"save","base":3,"content":"# Lesson\n"}
{"type":"cursor","cursor":42}
{"type":"ping"}
```

An `update` is broadcast to every socket, including the sender; `saved` and
`conflict` are sent only to the socket that attempted the save. The browser
uses `origin` to decide whether an update is its own acknowledgement or a
remote edit.

The server stores a complete snapshot of the last 256 room revisions. A save
with a stale base is merged three ways: base, the server's current text, and
the incoming text. If Git cannot merge cleanly, the server returns the current
text plus the conflicted merge and writes nothing.

## Docker

Build from the repository root, because the server shares `crates/core` with
the desktop app:

```sh
docker build -f server/Dockerfile -t markdownx-server .
docker run --rm -p 8877:8877 \
  -e MARKDOWNX_SERVER_TOKEN=change-me \
  -v "$PWD/data:/data" \
  markdownx-server
```

The image runs as UID 10001, stores the workspace in `/data`, and has a
healthcheck on `/api/health`.

## Company intranet

Point the server at the repository the team already clones:

```sh
markdownx-server -addr 0.0.0.0:8877 \
  -root /srv/team/design-repo \
  -token "$(openssl rand -hex 32)" \
  -origin https://docs.intra.example.com \
  -git-name "MarkdownX" -git-email "docs@intra.example.com"
```

Open `https://docs.intra.example.com/web/?token=…` once; the token is stored in
the browser and reused for HTTP and WebSocket. Put TLS in front (Caddy, nginx,
or an internal load balancer); `-origin` must match that public origin, and
WebSocket upgrade must be allowed. The process needs read/write access to the
repository and write access to Git's `user.name`/`user.email` (the server sets
local values when missing).

Recommended team layout:

```text
design-repo/
├── design/game/          # GDD, levels, systems
├── design/art/           # art direction and reviews
├── product/              # PRDs and roadmap
├── film/                 # treatments and shot lists
├── content/              # articles and campaigns
└── docs/                 # handbook and meeting notes
```

Use branches for large rewrites (`git switch -c feature/boss-fight`), and let
the server commit small changes on the current branch. The desktop app opens
the same repository because it widens any selected folder to the `.git` root.

## Security

- One shared bearer token. There are no user accounts, per-document ACLs or
  audit identities beyond the display name sent by the client.
- Put the server behind TLS and a reverse proxy; do not expose plain HTTP to
  the internet.
- The token can appear in WebSocket query strings and browser history; use
  the `mx_token` cookie or a reverse proxy that strips query logs if that
  matters.
- Document IDs are restricted; paths are resolved under `<root>` and Git
  arguments are passed without a shell.
- HTML widgets are not rendered by this server; it stores Markdown only.
- Back up `<root>` including `.git`; it is the entire workspace.

## Development

```sh
cargo test -p markdownx-server
cargo clippy -p markdownx-server --all-targets -- -D warnings
```

The browser editor is plain HTML/CSS/JS in `server/web`; it has no build step.
