package editor

import (
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
)

func newCore(text string) *Core {
	blocks := doc.MarkdownToBlocks(text, doc.Off{File: true})
	d := &doc.Document{Blocks: blocks}
	if len(blocks) == 0 {
		d = doc.NewDocument()
	}
	return New(d)
}

func firstBlock(c *Core) *doc.Block { return c.Doc.Blocks[0] }

func TestTypingInsertsAtCaret(t *testing.T) {
	c := newCore("")
	c.Sel = Caret(Pos{Block: firstBlock(c).ID})
	c.InsertText("hello")
	c.InsertText(" world")
	if got := firstBlock(c).Text.Text; got != "hello world" {
		t.Fatalf("text = %q", got)
	}
	if c.Sel.Head.Offset != 11 {
		t.Fatalf("caret = %d", c.Sel.Head.Offset)
	}
}

func TestEnterSplitsBlockAndCarriesKind(t *testing.T) {
	c := newCore("alpha beta")
	b := firstBlock(c)
	c.Sel = Caret(Pos{Block: b.ID, Offset: 5})
	c.Enter()
	if got := c.Doc.Blocks[0].Text.Text; got != "alpha" {
		t.Fatalf("first = %q", got)
	}
	if got := c.Doc.Blocks[1].Text.Text; got != " beta" {
		t.Fatalf("second = %q", got)
	}
	if c.Sel.Head.Block != c.Doc.Blocks[1].ID || c.Sel.Head.Offset != 0 {
		t.Fatalf("caret not at the start of the new block")
	}
}

func TestEnterOnEmptyListItemBecomesParagraph(t *testing.T) {
	c := newCore("- item\n- \n")
	// The second item is empty: Enter turns it into a paragraph.
	target := c.Doc.Blocks[1]
	c.Sel = Caret(Pos{Block: target.ID})
	c.Enter()
	if _, ok := c.Doc.Blocks[1].Kind.(doc.TextKind); !ok {
		t.Fatalf("kind = %T, want TextKind", c.Doc.Blocks[1].Kind)
	}
}

func TestBackspaceMergesIntoPreviousBlock(t *testing.T) {
	c := newCore("alpha\n\nbeta")
	second := c.Doc.Blocks[1]
	c.Sel = Caret(Pos{Block: second.ID})
	c.Backspace()
	if len(c.Doc.Blocks) != 1 {
		t.Fatalf("blocks = %d", len(c.Doc.Blocks))
	}
	if got := c.Doc.Blocks[0].Text.Text; got != "alphabeta" {
		t.Fatalf("merged = %q", got)
	}
	if c.Sel.Head.Offset != 5 {
		t.Fatalf("caret = %d", c.Sel.Head.Offset)
	}
}

func TestBackspaceOnNonParagraphDegradesToParagraph(t *testing.T) {
	c := newCore("## Heading")
	b := firstBlock(c)
	c.Sel = Caret(Pos{Block: b.ID})
	c.Backspace()
	if _, ok := c.Doc.Blocks[0].Kind.(doc.TextKind); !ok {
		t.Fatalf("kind = %T, want TextKind", c.Doc.Blocks[0].Kind)
	}
}

func TestUndoRedoRoundTrip(t *testing.T) {
	c := newCore("")
	c.Sel = Caret(Pos{Block: firstBlock(c).ID})
	c.InsertText("abc")
	c.BreakCoalescing()
	c.InsertText("def")
	if got := firstBlock(c).Text.Text; got != "abcdef" {
		t.Fatalf("text = %q", got)
	}
	if !c.Undo() {
		t.Fatal("undo failed")
	}
	if got := firstBlock(c).Text.Text; got != "abc" {
		t.Fatalf("after undo = %q", got)
	}
	if !c.Redo() {
		t.Fatal("redo failed")
	}
	if got := firstBlock(c).Text.Text; got != "abcdef" {
		t.Fatalf("after redo = %q", got)
	}
}

func TestToggleBoldOnSelection(t *testing.T) {
	c := newCore("hello world")
	b := firstBlock(c)
	c.Sel = Selection{Anchor: Pos{Block: b.ID, Offset: 0}, Head: Pos{Block: b.ID, Offset: 5}}
	c.ToggleMark(func(m *doc.Marks) bool { return m.Bold }, func(m *doc.Marks, on bool) { m.Bold = on })
	if len(firstBlock(c).Text.Runs) != 2 {
		t.Fatalf("runs = %d", len(firstBlock(c).Text.Runs))
	}
	if !firstBlock(c).Text.Runs[0].Marks.Bold {
		t.Fatal("first run should be bold")
	}
	// Toggling again clears it.
	c.ToggleMark(func(m *doc.Marks) bool { return m.Bold }, func(m *doc.Marks, on bool) { m.Bold = on })
	for _, r := range firstBlock(c).Text.Runs {
		if r.Marks.Bold {
			t.Fatal("bold should be cleared")
		}
	}
}

func TestHeadingShortcut(t *testing.T) {
	c := newCore("Title")
	c.Sel = Caret(Pos{Block: c.Doc.Blocks[0].ID})
	c.SetHeading(2)
	h, ok := c.Doc.Blocks[0].Kind.(doc.HeadingKind)
	if !ok || h.Level != 2 {
		t.Fatalf("kind = %#v", c.Doc.Blocks[0].Kind)
	}
	c.SetHeading(0)
	if _, ok := c.Doc.Blocks[0].Kind.(doc.TextKind); !ok {
		t.Fatalf("kind = %T", c.Doc.Blocks[0].Kind)
	}
}

func TestDuplicateBlocks(t *testing.T) {
	c := newCore("one\n\ntwo")
	first := c.Doc.Blocks[0]
	if _, ok := c.DuplicateBlocks([]doc.BlockID{first.ID}); !ok {
		t.Fatal("duplicate failed")
	}
	if len(c.Doc.Blocks) != 3 {
		t.Fatalf("blocks = %d", len(c.Doc.Blocks))
	}
	if c.Doc.Blocks[1].Text.Text != "one" {
		t.Fatalf("copy = %q", c.Doc.Blocks[1].Text.Text)
	}
	if c.Doc.Blocks[1].ID == first.ID {
		t.Fatal("copy should have a fresh id")
	}
}

func TestTodoToggle(t *testing.T) {
	c := newCore("- [ ] task")
	b := firstBlock(c)
	if todo, ok := b.Kind.(doc.TodoKind); !ok || todo.Done {
		t.Fatalf("kind = %#v", b.Kind)
	}
	c.ToggleTodoDone(b.ID)
	if todo, ok := c.Doc.Blocks[0].Kind.(doc.TodoKind); !ok || !todo.Done {
		t.Fatalf("kind = %#v", c.Doc.Blocks[0].Kind)
	}
}

func TestOutdentLiftsNestedBlock(t *testing.T) {
	c := newCore("- parent\n  - child")
	units := c.Units()
	var child doc.BlockID
	for _, u := range units {
		b := c.Block(u.ID)
		if b != nil {
			if _, ok := b.Kind.(doc.BulletKind); ok && b.Text.Text == "child" {
				child = u.ID
			}
		}
	}
	if child == 0 {
		t.Fatal("nested item not found")
	}
	c.Sel = Caret(Pos{Block: child})
	c.Outdent()
	if got := len(c.Doc.Blocks); got != 2 {
		t.Fatalf("top-level blocks = %d, want 2", got)
	}
}

func TestFoldHidesSectionFromUnits(t *testing.T) {
	c := newCore("## A\n\ntext\n\n## B")
	c.Doc.Blocks[0].Folded = true
	units := c.Units()
	var texts []string
	for _, u := range units {
		if b := c.Block(u.ID); b != nil {
			texts = append(texts, b.Text.Text)
		}
	}
	joined := strings.Join(texts, "|")
	if strings.Contains(joined, "text") {
		t.Fatalf("folded section still listed: %q", joined)
	}
	if !strings.Contains(joined, "B") {
		t.Fatalf("following heading missing: %q", joined)
	}
}

func TestDeleteSelectionAcrossBlocks(t *testing.T) {
	c := newCore("alpha\n\nbeta\n\ngamma")
	a := c.Doc.Blocks[0]
	g := c.Doc.Blocks[2]
	c.Sel = Selection{
		Anchor: Pos{Block: a.ID, Offset: 3},
		Head:   Pos{Block: g.ID, Offset: 2},
	}
	c.DeleteSelection()
	if len(c.Doc.Blocks) != 1 {
		t.Fatalf("blocks = %d", len(c.Doc.Blocks))
	}
	if got := c.Doc.Blocks[0].Text.Text; got != "alpmma" {
		t.Fatalf("text = %q", got)
	}
}

func TestMoveBlock(t *testing.T) {
	c := newCore("one\n\ntwo")
	one, two := c.Doc.Blocks[0], c.Doc.Blocks[1]
	if !c.MoveBlock(one.ID, two.ID, true) {
		t.Fatal("move failed")
	}
	if c.Doc.Blocks[0].ID != two.ID {
		t.Fatalf("order = %v, %v", c.Doc.Blocks[0].Text.Text, c.Doc.Blocks[1].Text.Text)
	}
}
