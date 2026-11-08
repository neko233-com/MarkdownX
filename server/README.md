# MarkdownX server

A small self-hosted server for MarkdownX books and Markdown documents:

- plain `.md` files under `<root>/docs`, written atomically;
- every accepted save is a Git commit;
- browser editor with live presence and conflict handling;
- history, unified diff and restore over HTTP;
- one static binary, no database.

It is intentionally small. It is not a CRDT service and it is not a multi-tenant
SaaS; read [Security](#security) before exposing it.

## Quick start

Requires Go 1.23+ and `git` on `PATH`.

```sh
cd server
go mod download
MARKDOWNX_SERVER_TOKEN=change-me go run ./cmd/markdownx-server \
  -root ./data -web ./web
```

Open <http://127.0.0.1:8787/> and enter the same token in the token dialog.

For local development, `-dev` disables the token requirement and allows any
browser origin:

```sh
go run ./cmd/markdownx-server -root ./.data -web ./web -dev
```

If the token is empty and `-dev` is not set, the server generates one at
startup and prints it to the log. Set `MARKDOWNX_SERVER_TOKEN` in production.

## Configuration

Flags override environment variables; both override defaults.

| Flag | Environment | Default | Meaning |
| --- | --- | --- | --- |
| `-addr` | `MARKDOWNX_SERVER_ADDR` | `:8787` | Listen address. |
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
<root>/
├── .git/                 # one commit per accepted save
├── .gitignore
└── docs/
    ├── lesson.md
    └── notes.md
```

Document IDs match `^[A-Za-z0-9][A-Za-z0-9._-]{0,96}$` and cannot contain
`..`. The title is the first `# ` heading, falling back to the ID. Saves use a
temporary file, `fsync`, then `rename`.

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
| `GET` | `/api/docs` | List documents. |
| `POST` | `/api/docs` | Create `{id?, title?, content?}`. |
| `GET` | `/api/docs/{id}` | Read `{id, title, content, doc}`. |
| `PUT` | `/api/docs/{id}` | Save `{base, content}`; `409` on conflict. |
| `GET` | `/api/docs/{id}/history?limit=50` | Git commits for the file. |
| `GET` | `/api/docs/{id}/diff?from=&to=` | Unified diff for a commit range. |
| `GET` | `/api/docs/{id}/revision/{rev}` | File content at a Git revision. |
| `POST` | `/api/docs/{id}/restore` | Restore `{rev: "<git-hash>"}` as a new commit. |
| `GET` | `/ws/docs/{id}` | WebSocket session. |

`base` is the room revision returned by the last `snapshot` or `saved`
message. On a stale base the server attempts a three-way merge with
`git merge-file`; the response is still `409` if the merge conflicts. Treat
`409` as "reload and reapply", not as a retry of the same bytes.

## WebSocket protocol

Connect to `/ws/docs/{id}?name=<display-name>&token=<token>`.

Server to client messages:

```json
{"type":"snapshot","clientId":"...","id":"lesson","title":"Lesson","rev":3,"content":"# Lesson\n","peers":[]}
{"type":"update","id":"lesson","rev":4,"title":"Lesson","by":"Reader-a1","at":"...","content":"# Lesson\n..."}
{"type":"saved","id":"lesson","rev":4,"changed":true,"title":"Lesson","commit":"<sha>"}
{"type":"conflict","id":"lesson","rev":4,"content":"...","merged":"...","by":"Reader-b2"}
{"type":"presence","peers":[{"id":"...","name":"Reader-a1","color":"#..."}]}
{"type":"error","message":"..."}
```

Client to server messages:

```json
{"type":"save","base":3,"content":"# Lesson\n..."}
{"type":"cursor","cursor":{"block":0,"offset":12}}
{"type":"ping"}
```

`saved.changed` is `false` when the content did not change. The room keeps the
last 256 revision snapshots in memory for stale-save merges. Revisions are
in-memory; after a restart the room starts from the file on disk at `rev: 1`.
Use Git hashes for durable history.

## Conflict model

The server is optimistic, not CRDT:

1. A client saves against a `base` revision.
2. If `base` is current, the server writes and commits.
3. If `base` is older, it loads that snapshot, runs a three-way merge against
   the current content, and either commits the clean merge or returns
   `conflict` without touching the file.
4. The browser offers "load latest", "use merged draft", or "overwrite remote".
   Overwrite rebases to the current revision before sending.

This keeps simultaneous edits in different regions and refuses to silently
drop text when edits overlap.

## Docker

```sh
docker build -t markdownx-server ./server
docker run --rm -p 8787:8787 \
  -e MARKDOWNX_SERVER_TOKEN=change-me \
  -v markdownx-data:/data \
  markdownx-server
```

The image runs as UID 10001 and stores data in `/data`. When using a bind mount,
make sure the host directory is writable by that UID.

The container includes `git`; the Git author can be set with
`MARKDOWNX_GIT_NAME` / `MARKDOWNX_GIT_EMAIL`.

## Security

- One shared token protects every document. There are no users, roles or
  per-document ACLs. Anyone with the token can read, edit, restore revisions
  and create new commits.
- Put the server behind TLS (a reverse proxy is fine). The server speaks plain
  HTTP unless you add TLS termination.
- Do not expose `-dev` to a network. It disables the token and loosens origin
  checks.
- The editor writes Markdown as plain text; it does not render embedded HTML.
  Still treat the token as a write credential.
- `-max-bytes` bounds a single document, not total disk use. Git history grows
  with edits; back up `<root>` and prune the repository deliberately.
- The server binds to the address you give it. `:8787` listens on all
  interfaces; use `127.0.0.1:8787` for a local-only instance.

## Tests

```sh
go test ./...
go vet ./...
```

The tests cover storage and Git commits, stale-save merges, conflict handling,
HTTP auth/validation, and a real WebSocket save/broadcast round trip.
