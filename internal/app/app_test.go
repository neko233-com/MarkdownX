package app

import (
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func newTestApp(text string) *App {
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(text, doc.Off{File: true})})
	a.ResetBuffers()
	return a
}

func TestFindMatchesAreCaseInsensitive(t *testing.T) {
	a := newTestApp("alpha BETA\n\ngamma beta")
	hits := a.findMatches("beta")
	if len(hits) != 2 {
		t.Fatalf("hits = %d, want 2", len(hits))
	}
	if hits[0].end-hits[0].start != 4 {
		t.Fatalf("match width = %d", hits[0].end-hits[0].start)
	}
}

func TestFindMatchesInsideNestedBlocks(t *testing.T) {
	a := newTestApp("- parent target\n  - child target")
	hits := a.findMatches("target")
	if len(hits) != 2 {
		t.Fatalf("hits = %d, want 2", len(hits))
	}
}

func TestFindMatchesEmptyQuery(t *testing.T) {
	a := newTestApp("anything")
	if hits := a.findMatches("   "); len(hits) != 0 {
		t.Fatalf("hits = %d", len(hits))
	}
}

func TestReplaceAll(t *testing.T) {
	a := newTestApp("one two one one")
	a.findQuery = "one"
	a.refreshMatches()
	a.replaceAll("1")
	if got := a.core.Doc.PlainText(); got != "1 two 1 1" {
		t.Fatalf("text = %q", got)
	}
}

func TestReplaceOne(t *testing.T) {
	a := newTestApp("x y x")
	a.findQuery = "x"
	a.refreshMatches()
	a.replaceOne("z")
	if got := a.core.Doc.PlainText(); got != "z y x" {
		t.Fatalf("text = %q", got)
	}
}

func TestDisplayName(t *testing.T) {
	a := newTestApp("")
	if got := a.DisplayName(); got != "未命名" {
		t.Fatalf("name = %q", got)
	}
	*a.title = "笔记"
	if got := a.DisplayName(); got != "笔记" {
		t.Fatalf("name = %q", got)
	}
	a.path = "/tmp/note.md"
	if got := a.DisplayName(); got != "note.md" {
		t.Fatalf("name = %q", got)
	}
}

func TestStatsCountsCharacters(t *testing.T) {
	a := newTestApp("中文 ab")
	if got := a.Stats(); got != "5 字" {
		t.Fatalf("stats = %q", got)
	}
}

func TestInsertTableRoundTrips(t *testing.T) {
	a := newTestApp("before")
	a.focus = a.core.Doc.Blocks[0].ID
	a.insertTable()
	a.ResetBuffers()

	// The table is the second block, followed by an empty paragraph.
	if len(a.core.Doc.Blocks) != 3 {
		t.Fatalf("blocks = %d, want 3", len(a.core.Doc.Blocks))
	}
	tk, ok := a.core.Doc.Blocks[1].Kind.(doc.TableKind)
	if !ok {
		t.Fatalf("second block is %T", a.core.Doc.Blocks[1].Kind)
	}
	if got := len(tk.Table.Rows); got != 3 {
		t.Fatalf("rows = %d", got)
	}
	if got := tk.Table.NCols(); got != 3 {
		t.Fatalf("cols = %d", got)
	}
	// Fill a cell and check it reaches the Markdown.
	tk.Table.Rows[0][0].Blocks[0].Text = doc.PlainRich("h1")
	tk.Table.Rows[1][0].Blocks[0].Text = doc.PlainRich("v1")
	md := doc.Export(a.core.Doc, doc.Options{}).Text
	for _, want := range []string{"| h1 |  |  |", "| v1 |  |  |"} {
		if !strings.Contains(md, want) {
			t.Errorf("missing %q in:\n%s", want, md)
		}
	}
}

func TestInsertBlockKeepsATrailingParagraph(t *testing.T) {
	a := newTestApp("text")
	a.focus = a.core.Doc.Blocks[0].ID
	a.insertBlock(doc.NewBlock(doc.DividerKind{}, doc.RichText{}))
	last := a.core.Doc.Blocks[len(a.core.Doc.Blocks)-1]
	if _, ok := last.Kind.(doc.TextKind); !ok {
		t.Fatalf("last block is %T, want TextKind", last.Kind)
	}
	if a.focus != last.ID {
		t.Fatal("focus should move to the new paragraph")
	}
}
