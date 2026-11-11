# Team workflows

MarkdownX treats the repository as the workspace. A designer, PM, writer and
engineer can work in the same folder tree, on the same branches, without a
second content system.

## The workspace

Open a subfolder with **File → Open Folder…**. If Git can resolve a parent,
MarkdownX widens the tree to the repository root (`git rev-parse
--show-toplevel`). Desktop file saves, folder searches and link indexing run in the background. Empty files open with a ready-to-type body. Unchanged source fragments retain their original Markdown syntax and line endings. A save that cannot preserve the edited content or formatting is stopped and offers a recovery copy; external changes are not overwritten.
The desktop and collaboration server use isolated Git staging for automatic
commits; if the touched file already has staged changes, the file remains saved
and the commit is skipped with a warning. A server edit conflicting with an
external disk change offers both versions for resolution instead of silently
overwriting the file.

```text
studio-repo/
├── .git/
├── design/
│   ├── game/            # GDD, systems, levels, economy
│   ├── narrative/       # story, quests, dialogue
│   └── art/             # art direction, reviews
├── product/             # PRDs, research, roadmap
├── film/                # treatments, scripts, shot lists
├── content/             # articles, campaigns, calendar
├── docs/                # handbook, meeting notes, decisions
└── templates/           # starters used by the server and agents
```

The collaboration server points at the same repository:

```sh
markdownx-server -addr 0.0.0.0:8877 -root /srv/studio-repo \
  -token "$(openssl rand -hex 32)" -origin https://docs.studio.example
```

The browser tree lists nested `.md` paths and Chinese filenames. The desktop
tree also shows those paths, excluding dot-prefixed entries and generated folders. Both commit to the current Git
branch, so a code review or `git log` is the audit trail.

## Branching

- `main`: reviewed, published design.
- `feature/<slug>`: a level, a pitch, a campaign, a sprint of docs.
- `review/<name>`: optional editorial pass before merge.

Small edits commit directly. Large rewrites should happen on a branch; open the
same folder in the desktop app or the browser and switch branches with Git.
The server does not create or merge branches for you.

## Game design

Templates: `game-design`, `level-design`, `narrative`.

1. Start a GDD from `templates/game-design.md`.
2. Link systems and levels with `[[combat]]`, `[[boss-fight]]`.
3. Put tunable Canvas blocks in the design doc: a damage curve, an economy
   projection, a level pacing graph.
4. Review with annotations; resolve them in batch when the milestone closes.
5. Export a `.mdxb` build for playtest review, or let the team read the same
   Markdown in the browser.

The GDD is not a wiki page frozen in a tool. It is a folder in the same repo as
the code and the art.

## Product

Templates: `product-prd`, `meeting-notes`.

Use `product/prd/<initiative>.md` for the problem, scope and acceptance
criteria; put decisions in `docs/decisions/YYYY-MM-DD-topic.md`. A decision
record should state the choice, the alternatives and the cost. Link it from the
PRD with `[[decisions/...]]` so the backlink panel shows every affected spec.

## Film and video

Templates: `film-treatment`.

Keep a treatment in `film/treatment.md`, a shot list in `film/shots.md`, and
continuity notes in `film/continuity.md`. Tables carry shot number, framing,
sound and duration. Attach storyboard images next to the Markdown and reference
them with `assets/...`; the folder stays portable.

## Content and editorial

Templates: `content-article`.

A brief lives in `content/briefs/`, the draft in `content/drafts/`, and the
calendar in `content/calendar.md`. Use annotations for editor comments and
`@name` to assign a section. Git history gives the fact-check trail without a
separate CMS.

## Local and online

- **Local**: desktop app, Git on disk, no account. Good for flights, focus work
  and private drafts.
- **Online**: the Rust server over the company intranet, same repository, live
  presence and three-way merge. Good for review rooms and co-writing.
- **Handoff**: `.mdxb` for a read-only book build; Markdown folders for a
  working source tree; Git for everything in between.

The browser and desktop do not share a live session yet. They share the files,
the Git history and the format. The desktop has a reserved collaboration core;
the browser editor is the working real-time surface today.

## Etiquette

- One topic per file; link instead of copying.
- Write the decision, not only the discussion.
- Keep templates in `templates/`; do not fork them per team.
- Run the server's `check` equivalent before publishing a `.mdxb` build.
- Do not put secrets in HTML widgets or in the repository.
