package editor

import (
	"unicode"
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/doc"
)

// SyncText takes an edit from a native text field, keeping the surrounding runs.
func (c *Core) SyncText(id doc.BlockID, text string) (start, end int, changed bool) {
	t := c.TextOf(id)
	if t == nil || t.Text == text {
		return 0, 0, false
	}
	next := *t
	start, end = next.Resync(text)
	c.SyncRichText(id, next)
	return start, end, true
}

// SyncRichText commits a native attributed-text edit.
func (c *Core) SyncRichText(id doc.BlockID, next doc.RichText) {
	t := c.TextOf(id)
	if t == nil {
		return
	}
	c.Checkpoint(EditInsert)
	if b := c.Block(id); b != nil {
		switch b.Kind.(type) {
		case doc.CodeKind, doc.DefinitionKind:
			next = doc.PlainRich(next.Text)
			c.PendingMk = nil
		}
	}
	if marks := c.PendingMk; marks != nil && next.Text != t.Text {
		probe := *t
		start, end := probe.Resync(next.Text)
		if start < end {
			next.UpdateMarks(start, end, func(m *doc.Marks) { *m = *marks })
			c.PendingMk = nil
		}
	}
	*t = next
	c.touch()
}

// HardBreak inserts a line break inside the current paragraph.
func (c *Core) HardBreak() {
	c.Checkpoint(EditOther)
	c.DeleteSelection()
	p := c.Sel.Head
	if t := c.TextOf(p.Block); t != nil {
		t.InsertPlain(p.Offset, "\n", doc.Marks{})
		p.Offset++
		c.Sel = Caret(p)
		c.touch()
	}
}

// InsertPlainText inserts a clipboard value without Markdown interpretation.
func (c *Core) InsertPlainText(text string) {
	if text == "" || c.TextOf(c.Sel.Head.Block) == nil {
		return
	}
	c.Checkpoint(EditOther)
	c.DeleteSelection()
	p := c.Sel.Head
	t := c.TextOf(p.Block)
	t.InsertPlain(p.Offset, text, doc.Marks{})
	c.Sel = Caret(Pos{Block: p.Block, Offset: p.Offset + len(text)})
	c.PendingMk = nil
	c.touch()
}

// SelectWord expands the caret to the word at its edge in this text unit.
func (c *Core) SelectWord() bool {
	p := c.Sel.Head
	t := c.TextOf(p.Block)
	if t == nil {
		return false
	}
	start, end := WordBounds(t.Text, p.Offset)
	if start == end {
		return false
	}
	c.Sel = Selection{Anchor: Pos{p.Block, start}, Head: Pos{p.Block, end}}
	return true
}

// WordBounds returns the byte range of the word across or beside the caret.
func WordBounds(text string, at int) (start, end int) {
	isWord := func(r rune) bool {
		return unicode.IsLetter(r) || unicode.IsNumber(r) || unicode.IsMark(r) || r == '_'
	}
	start, end = at, at
	for start > 0 {
		r, n := utf8.DecodeLastRuneInString(text[:start])
		if !isWord(r) {
			break
		}
		start -= n
	}
	for end < len(text) {
		r, n := utf8.DecodeRuneInString(text[end:])
		if !isWord(r) {
			break
		}
		end += n
	}
	return start, end
}

// ReplaceDocument applies an edit made in the Markdown source view.
func (c *Core) ReplaceDocument(d *doc.Document, caret Pos) {
	c.Checkpoint(EditOther)
	c.Doc = d
	c.Sel = Caret(caret)
	c.PendingMk = nil
	c.touch()
}

// FormatPrefix consumes a typed block marker and places the caret in its text.
func (c *Core) FormatPrefix(id doc.BlockID, kind doc.BlockKind, markerLen int, rest string) Pos {
	b := c.Block(id)
	c.Checkpoint(EditOther)
	text := b.Text.Slice(markerLen, b.Text.Len())
	if _, code := kind.(doc.CodeKind); code {
		text = doc.PlainRich(rest)
	}
	b.Kind = kind
	b.Text = text
	target := b
	if _, quote := kind.(doc.QuoteKind); quote {
		target = doc.NewBlock(doc.TextKind{}, text)
		b.Text = doc.RichText{}
		b.Children = append([]*doc.Block{target}, b.Children...)
	}
	p := Pos{Block: target.ID, Offset: target.Text.Len()}
	c.Sel = Caret(p)
	c.touch()
	return p
}

// FormatInline replaces completed Markdown syntax with rich text in one step.
func (c *Core) FormatInline(id doc.BlockID, start, end int, text doc.RichText) Pos {
	c.Checkpoint(EditOther)
	c.TextOf(id).Replace(start, end, text)
	p := Pos{Block: id, Offset: start + text.Len()}
	c.Sel = Caret(p)
	c.touch()
	return p
}

// InsertBlocks adds a group, such as a table and its following paragraph.
func (c *Core) InsertBlocks(id doc.BlockID, blocks ...*doc.Block) {
	if len(blocks) == 0 {
		return
	}
	c.Checkpoint(EditOther)
	for _, b := range blocks {
		if id == Title {
			c.Doc.Blocks = append(c.Doc.Blocks, b)
		} else {
			c.InsertAfter(id, b)
		}
		id = b.ID
	}
	c.Sel = Caret(Pos{Block: id})
	c.touch()
}

// ReplaceBlock turns a Markdown line into a structured block with an optional
// paragraph after it. Both changes form one undo step.
func (c *Core) ReplaceBlock(id doc.BlockID, b *doc.Block, next bool) Pos {
	c.Checkpoint(EditOther)
	b.ID = id
	blocks, index := c.siblingList(id)
	(*blocks)[index] = b
	if next {
		b = doc.TextBlock("")
		c.InsertAfter(id, b)
	}
	c.Sel = Caret(Pos{Block: b.ID})
	c.touch()
	return c.Sel.Head
}
