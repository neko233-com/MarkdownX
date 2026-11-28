package editor

import (
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
)

func TestDocumentRangeRemovesAnInteriorTableAsAUnit(t *testing.T) {
	c := newCore("甲alpha\n\n| A | B |\n| --- | --- |\n| C | D |\n\n**end**尾")
	left, table, right := c.Doc.Blocks[0], c.Doc.Blocks[1], c.Doc.Blocks[2]
	if _, ok := table.Kind.(doc.TableKind); !ok {
		t.Fatal("fixture has no table")
	}
	c.Sel = Selection{Anchor: Pos{right.ID, len("e")}, Head: Pos{left.ID, len("甲")}}
	selected, _ := c.SelectedDocument().WorkspaceMarkdown()
	if !strings.Contains(selected, "| C | D |") || !strings.Contains(selected, "**e**") {
		t.Fatalf("copy lost the table or marks: %q", selected)
	}
	c.ReplaceSelection(doc.PlainRich("🙂"))
	if len(c.Doc.Blocks) != 1 || left.Text.Text != "甲🙂nd尾" || !left.Text.MarksAt(len("甲🙂n")).Bold {
		t.Fatalf("replacement lost the unselected suffix: %+v", left.Text)
	}
	if !c.Undo() || len(c.Doc.Blocks) != 3 || !c.Sel.AcrossBlocks() || c.Doc.Blocks[1].ID != table.ID {
		t.Fatal("undo did not restore the table and the original selection")
	}
}

func TestDocumentRangePreservesPartialTableAndNestedSuffix(t *testing.T) {
	c := newCore("left\n\n| A | B | C |\n| --- | --- | --- |\n| row | keep | tail |")
	left := c.Doc.Blocks[0]
	table := c.Doc.Blocks[1].Kind.(doc.TableKind).Table
	end := table.Rows[1][1].Blocks[0]
	c.Sel = Selection{Anchor: Pos{left.ID, 2}, Head: Pos{end.ID, 2}}
	c.DeleteSelection()
	if len(c.Doc.Blocks) != 2 || table.NCols() != 3 || len(table.Rows) != 2 ||
		len(table.Rows[0][0].Blocks) != 1 || table.Rows[0][0].Blocks[0].Text.Text != "" ||
		end.Text.Text != "ep" || table.Rows[1][2].Blocks[0].Text.Text != "tail" {
		t.Fatalf("partial deletion damaged the table: %+v", table)
	}

	nested := doc.TextBlock("grandchild")
	parent := doc.NewBlock(doc.BulletKind{}, doc.PlainRich("parent")).WithChildren(nested)
	c = New(&doc.Document{Blocks: []*doc.Block{doc.TextBlock("left"), parent, doc.TextBlock("after")}})
	c.Sel = Selection{Anchor: Pos{c.Doc.Blocks[0].ID, 2}, Head: Pos{parent.ID, 2}}
	c.DeleteSelection()
	if len(c.Doc.Blocks) != 3 || parent.Text.Text != "rent" || len(parent.Children) != 1 || parent.Children[0] != nested {
		t.Fatalf("nested suffix was lost: %+v", parent)
	}
}

func TestDocumentSelectAllCoversFoldedBlocksAndCode(t *testing.T) {
	c := newCore("# Heading\n\nText\n\n```text\ncode\n```\n\n---\n")
	first, hidden, code := c.Doc.Blocks[0], c.Doc.Blocks[1], c.Doc.Blocks[2]
	first.Folded = true
	c.Sel = Selection{Anchor: Pos{Block: first.ID}, Head: Pos{first.ID, first.Text.Len()}, All: true}
	c.ToggleMark(func(m *doc.Marks) bool { return m.Bold }, func(m *doc.Marks, on bool) { m.Bold = on })
	if !hidden.Text.MarksAt(1).Bold || code.Text.MarksAt(1).Bold {
		t.Fatal("formatting omitted folded text or changed literal code")
	}
	c.DeleteForward()
	if !c.Doc.IsBlank() || !c.Sel.Collapsed() || c.Sel.Head.Block != c.Doc.Blocks[0].ID {
		t.Fatal("select-all delete did not leave one editable paragraph")
	}
	if !c.Undo() || len(c.Doc.Blocks) != 4 || !c.Sel.All {
		t.Fatal("undo did not restore folded and atomic blocks")
	}
}
