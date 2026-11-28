package editor

import (
	"slices"

	"github.com/neko233/markdownx/internal/doc"
)

// Span is a selected byte range in a single text block.
type Span struct{ Start, End int }

type blockRange struct{ first, last int }

type documentRange struct {
	from, to    Pos
	first, last int
	blocks      map[doc.BlockID]blockRange
}

func (c *Core) documentRange() (documentRange, bool) {
	r := documentRange{from: c.Sel.Anchor, to: c.Sel.Head, blocks: map[doc.BlockID]blockRange{Title: {}}}
	index := 0
	var walk func([]*doc.Block)
	walk = func(blocks []*doc.Block) {
		for _, b := range blocks {
			index++
			first := index
			walk(b.Children)
			if tk, ok := b.Kind.(doc.TableKind); ok && tk.Table != nil {
				for _, row := range tk.Table.Rows {
					for _, cell := range row {
						walk(cell.Blocks)
					}
				}
			}
			r.blocks[b.ID] = blockRange{first, index}
		}
	}
	walk(c.Doc.Blocks)
	if c.Sel.All {
		r.from, r.to = Pos{Block: Title}, Pos{}
		r.first, r.last = 0, index+1
		return r, true
	}
	start, haveStart := r.blocks[r.from.Block]
	end, haveEnd := r.blocks[r.to.Block]
	if start.first > end.first || (start.first == end.first && r.from.Offset > r.to.Offset) {
		r.from, r.to = r.to, r.from
		start, end = end, start
	}
	r.first, r.last = start.first, end.first
	return r, haveStart && haveEnd
}

func (r documentRange) span(id doc.BlockID, length int) (Span, bool) {
	n, ok := r.blocks[id]
	if !ok || n.first < r.first || n.first > r.last {
		return Span{}, false
	}
	s := Span{End: length}
	if n.first == r.first {
		s.Start = r.from.Offset
	}
	if n.first == r.last {
		s.End = r.to.Offset
	}
	return s, true
}

// SelectionSpans includes the selected text and the selected atomic objects.
func (c *Core) SelectionSpans() map[doc.BlockID]Span {
	spans := make(map[doc.BlockID]Span)
	r, ok := c.documentRange()
	if !ok || c.Sel.Collapsed() {
		return spans
	}
	if span, ok := r.span(Title, c.Doc.Title.Len()); ok {
		spans[Title] = span
	}
	c.Doc.ForEachBlock(func(b *doc.Block) {
		if _, table := b.Kind.(doc.TableKind); table || b.Kind.IsContainer() {
			return
		}
		if span, ok := r.span(b.ID, b.Text.Len()); ok {
			spans[b.ID] = span
		}
	})
	return spans
}

// SelectedDocument keeps the block structure and inline styles of a range.
func (c *Core) SelectedDocument() *doc.Document {
	if c.Sel.All {
		return c.Doc.Clone()
	}
	d := &doc.Document{}
	r, ok := c.documentRange()
	if !ok || c.Sel.Collapsed() {
		return d
	}
	if span, ok := r.span(Title, c.Doc.Title.Len()); ok {
		d.Title = c.Doc.Title.Slice(span.Start, span.End)
	}
	var copyBlocks func([]*doc.Block) []*doc.Block
	copyBlocks = func(blocks []*doc.Block) []*doc.Block {
		var out []*doc.Block
		for _, b := range blocks {
			n := r.blocks[b.ID]
			if n.last < r.first || n.first > r.last {
				continue
			}
			if n.first > r.first && n.last < r.last {
				out = append(out, b.Clone())
				continue
			}
			copy := b.Clone()
			copy.Children = copyBlocks(b.Children)
			if span, ok := r.span(b.ID, b.Text.Len()); ok {
				copy.Text = b.Text.Slice(span.Start, span.End)
			} else {
				copy.Text = doc.RichText{}
			}
			if tk, ok := copy.Kind.(doc.TableKind); ok && tk.Table != nil {
				original := b.Kind.(doc.TableKind).Table
				for i, row := range original.Rows {
					for j, cell := range row {
						tk.Table.Rows[i][j].Blocks = copyBlocks(cell.Blocks)
					}
				}
			}
			out = append(out, copy)
		}
		return out
	}
	d.Blocks = copyBlocks(c.Doc.Blocks)
	return d
}

func (c *Core) deleteDocumentRange() {
	if c.Sel.All {
		b := doc.TextBlock("")
		c.Doc = &doc.Document{Blocks: []*doc.Block{b}, Icon: c.Doc.Icon, Cover: c.Doc.Cover}
		c.Sel = Caret(Pos{Block: b.ID})
		c.touch()
		return
	}
	r, ok := c.documentRange()
	if !ok {
		return
	}
	left, right := c.Block(r.from.Block), c.Block(r.to.Block)
	join := false
	if left != nil && right != nil && left.Kind.HasText() && right.Kind.HasText() {
		lp, _ := FindPath(c.Doc.Blocks, left.ID)
		rp, _ := FindPath(c.Doc.Blocks, right.ID)
		_, leftCode := left.Kind.(doc.CodeKind)
		_, rightCode := right.Kind.(doc.CodeKind)
		join = leftCode == rightCode && slices.Equal(lp[:len(lp)-1], rp[:len(rp)-1])
	}
	if r.from.Block == Title {
		c.Doc.Title = c.Doc.Title.Slice(0, r.from.Offset)
	}
	var remove func([]*doc.Block) []*doc.Block
	remove = func(blocks []*doc.Block) []*doc.Block {
		out := blocks[:0]
		for _, b := range blocks {
			n := r.blocks[b.ID]
			if n.last < r.first || n.first > r.last {
				out = append(out, b)
				continue
			}
			if n.first > r.first && n.last < r.last {
				continue
			}
			b.Children = remove(b.Children)
			if tk, ok := b.Kind.(doc.TableKind); ok && tk.Table != nil {
				for i, row := range tk.Table.Rows {
					for j, cell := range row {
						cell.Blocks = remove(cell.Blocks)
						if len(cell.Blocks) == 0 && !cell.IsCovered() {
							cell.Blocks = []*doc.Block{doc.TextBlock("")}
						}
						tk.Table.Rows[i][j] = cell
					}
				}
			}
			switch b.ID {
			case r.from.Block:
				b.Text = b.Text.Slice(0, r.from.Offset)
			case r.to.Block:
				b.Text = b.Text.Slice(r.to.Offset, b.Text.Len())
				if join && len(b.Children) == 0 {
					left.Text.Append(b.Text)
					continue
				}
			default:
				if n.first > r.first && n.first < r.last {
					b.Text = doc.RichText{}
				}
			}
			out = append(out, b)
		}
		return out
	}
	c.Doc.Blocks = remove(c.Doc.Blocks)
	c.Sel = Caret(r.from)
	c.touch()
}

// ReplaceSelection inserts attributed text as one undoable edit.
func (c *Core) ReplaceSelection(text doc.RichText) {
	c.Checkpoint(EditOther)
	c.DeleteSelection()
	p := c.Sel.Head
	if t := c.TextOf(p.Block); t != nil && !text.IsEmpty() {
		if b := c.Block(p.Block); b != nil {
			if _, code := b.Kind.(doc.CodeKind); code {
				text = doc.PlainRich(text.PlainText())
			}
		}
		t.Replace(p.Offset, p.Offset, text)
		p.Offset += text.Len()
		c.Sel = Caret(p)
		c.touch()
	}
	c.PendingMk = nil
}

// PasteBlocks inserts a clipboard fragment in one undo step. Literal code
// and table cells keep their editor's inline paste behavior.
func (c *Core) PasteBlocks(blocks []*doc.Block) bool {
	if len(blocks) == 0 {
		return false
	}
	if !c.Sel.All {
		start, _ := c.Ordered()
		b := c.Block(start.Block)
		if b == nil || !b.Kind.HasText() {
			return false
		}
		if _, code := b.Kind.(doc.CodeKind); code {
			return false
		}
		if _, cell := c.TableCellAt(b.ID); cell {
			return false
		}
	}
	plainParagraph := func(b *doc.Block) bool {
		_, plain := b.Kind.(doc.TextKind)
		return plain && len(b.Children) == 0 && b.Align == doc.AlignLeft && b.BG == nil
	}
	if len(blocks) == 1 && plainParagraph(blocks[0]) {
		c.ReplaceSelection(blocks[0].Text)
		return true
	}
	fragment := make([]*doc.Block, len(blocks))
	for i, b := range blocks {
		fragment[i] = b.FreshIDs()
	}
	c.Checkpoint(EditOther)
	c.DeleteSelection()
	p := c.Sel.Head
	target := c.Block(p.Block)
	list, index := c.siblingList(target.ID)
	prefix, suffix := target.Text.Slice(0, p.Offset), target.Text.Slice(p.Offset, target.Text.Len())
	first := fragment[0]
	if !prefix.IsEmpty() && plainParagraph(target) && plainParagraph(first) {
		prefix.Append(first.Text)
		target.Text = prefix
		fragment[0] = target
	} else if !prefix.IsEmpty() || !plainParagraph(target) {
		target.Text = prefix
		fragment = append([]*doc.Block{target}, fragment...)
	}
	last := fragment[len(fragment)-1]
	p = Pos{Block: last.ID, Offset: last.Text.Len()}
	switch {
	case plainParagraph(last):
		last.Text.Append(suffix)
	case !suffix.IsEmpty() || !last.Kind.HasText():
		paragraph := doc.NewBlock(doc.TextKind{}, suffix)
		fragment = append(fragment, paragraph)
		p = Pos{Block: paragraph.ID}
	}
	*list = slices.Replace(*list, index, index+1, fragment...)
	c.Sel = Caret(p)
	c.PendingMk = nil
	c.touch()
	return true
}
