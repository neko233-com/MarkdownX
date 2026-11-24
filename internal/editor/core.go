// Package editor is MarkdownX's block editor: a document, a selection, and
// the edits the keyboard and menus perform on them.
package editor

import (
	"strings"
	"unicode/utf8"

	"github.com/neko233/markdownx/internal/doc"
)

// TITLE is the pseudo block that holds the document title.
const Title doc.BlockID = 0

// MaxNestDepth is the deepest a text block may sit below one container.
const MaxNestDepth = 16

// Pos is a caret: a block and a byte offset into its text.
type Pos struct {
	Block  doc.BlockID
	Offset int
}

// Selection is an anchor and a head; equal means a collapsed caret.
type Selection struct {
	Anchor Pos
	Head   Pos
}

// Caret makes a collapsed selection.
func Caret(p Pos) Selection { return Selection{Anchor: p, Head: p} }

// Collapsed reports whether nothing is selected.
func (s Selection) Collapsed() bool { return s.Anchor == s.Head }

// Seg is one step of a block path: a child index, or a table cell's block.
type Seg struct {
	IsCell  bool
	R, C    int
	Index   int
}

// Path locates a block from the document root.
type Path []Seg

// Unit is one caret stop in document order.
type Unit struct {
	ID    doc.BlockID
	Text  bool
	Scope doc.BlockID
}

// FindPath locates a block by id.
func FindPath(blocks []*doc.Block, id doc.BlockID) (Path, bool) {
	for i, b := range blocks {
		if b.ID == id {
			return Path{{Index: i}}, true
		}
		if sub, ok := FindPath(b.Children, id); ok {
			return append(Path{{Index: i}}, sub...), true
		}
		if tk, ok := b.Kind.(doc.TableKind); ok && tk.Table != nil {
			for r := range tk.Table.Rows {
				for c := range tk.Table.Rows[r] {
					if sub, ok := FindPath(tk.Table.Rows[r][c].Blocks, id); ok {
						return append(Path{{Index: i}, {IsCell: true, R: r, C: c}}, sub...), true
					}
				}
			}
		}
	}
	return nil, false
}

// BlockAt resolves a path to a block.
func BlockAt(blocks []*doc.Block, p Path) *doc.Block {
	if len(p) == 0 {
		return nil
	}
	cur := p[0]
	if cur.IsCell {
		return nil
	}
	if cur.Index < 0 || cur.Index >= len(blocks) {
		return nil
	}
	b := blocks[cur.Index]
	rest := p[1:]
	if len(rest) == 0 {
		return b
	}
	if rest[0].IsCell {
		tk, ok := b.Kind.(doc.TableKind)
		if !ok || tk.Table == nil {
			return nil
		}
		cell := rest[0]
		if cell.R < 0 || cell.R >= len(tk.Table.Rows) || cell.C < 0 || cell.C >= len(tk.Table.Rows[cell.R]) {
			return nil
		}
		return BlockAt(tk.Table.Rows[cell.R][cell.C].Blocks, rest[1:])
	}
	return BlockAt(b.Children, rest)
}

// Units lists every caret stop in document order, hiding folded sections.
func Units(d *doc.Document) []Unit {
	out := []Unit{{ID: Title, Text: true}}
	var walk func(blocks []*doc.Block, scope doc.BlockID)
	walk = func(blocks []*doc.Block, scope doc.BlockID) {
		hideBelow := -1
		for _, b := range blocks {
			if hideBelow >= 0 {
				if h, ok := b.Kind.(doc.HeadingKind); ok && int(h.Level) <= hideBelow {
					hideBelow = -1
				} else {
					continue
				}
			}
			if b.Kind.HasText() {
				out = append(out, Unit{ID: b.ID, Text: true, Scope: scope})
				if h, ok := b.Kind.(doc.HeadingKind); ok && b.Folded {
					hideBelow = int(h.Level)
				}
				if !b.Folded {
					walk(b.Children, scope)
				}
				continue
			}
			if b.Kind.IsContainer() {
				walk(b.Children, scope)
				continue
			}
			if tk, ok := b.Kind.(doc.TableKind); ok && tk.Table != nil {
				for r := range tk.Table.Rows {
					for c := range tk.Table.Rows[r] {
						walk(tk.Table.Rows[r][c].Blocks, b.ID)
					}
				}
				continue
			}
			out = append(out, Unit{ID: b.ID, Scope: scope})
		}
	}
	walk(d.Blocks, 0)
	return out
}

// PrevBoundary moves an offset back one rune.
func PrevBoundary(s string, offset int) int {
	if offset <= 0 {
		return 0
	}
	_, size := utf8.DecodeLastRuneInString(s[:offset])
	return offset - size
}

// NextBoundary moves an offset forward one rune.
func NextBoundary(s string, offset int) int {
	if offset >= len(s) {
		return len(s)
	}
	_, size := utf8.DecodeRuneInString(s[offset:])
	return offset + size
}

// EditKind names an undo step for coalescing.
type EditKind int

const (
	EditOther EditKind = iota
	EditInsert
	EditDelete
)

type snapshot struct {
	doc doc.Document
	sel Selection
}

// Core is the editor: document, selection, undo history.
type Core struct {
	Doc        *doc.Document
	Sel        Selection
	PendingMk  *doc.Marks
	Dirty      bool
	Generation uint64
	Notice     string

	undo     []snapshot
	redo     []snapshot
	lastKind EditKind
}

// New makes an editor over a document, with the caret in the title when the
// title is empty.
func New(d *doc.Document) *Core {
	c := &Core{Doc: d}
	start := Title
	if d.Title.Text != "" {
		if us := Units(d); len(us) > 1 {
			start = us[1].ID
		}
	}
	c.Sel = Caret(Pos{Block: start})
	return c
}

// Path locates a block.
func (c *Core) Path(id doc.BlockID) (Path, bool) {
	if id == Title {
		return nil, false
	}
	return FindPath(c.Doc.Blocks, id)
}

// Block resolves an id to a block.
func (c *Core) Block(id doc.BlockID) *doc.Block {
	p, ok := c.Path(id)
	if !ok {
		return nil
	}
	return BlockAt(c.Doc.Blocks, p)
}

// TextOf returns a block's caret text.
func (c *Core) TextOf(id doc.BlockID) *doc.RichText {
	if id == Title {
		return &c.Doc.Title
	}
	b := c.Block(id)
	if b == nil || !b.Kind.HasText() {
		return nil
	}
	return &b.Text
}

// LenOf is the byte length of a block's text.
func (c *Core) LenOf(id doc.BlockID) int {
	if t := c.TextOf(id); t != nil {
		return t.Len()
	}
	return 0
}

// Units lists the caret stops.
func (c *Core) Units() []Unit { return Units(c.Doc) }

func (c *Core) touch() {
	c.Generation++
	c.Dirty = true
}

// Checkpoint records an undo step.
func (c *Core) Checkpoint(kind EditKind) {
	if kind != c.lastKind || kind != EditInsert {
		c.undo = append(c.undo, snapshot{doc: *c.Doc.Clone(), sel: c.Sel})
		if len(c.undo) > 256 {
			c.undo = c.undo[1:]
		}
		c.redo = nil
	}
	c.lastKind = kind
}

// BreakCoalescing ends a run of typing so the next edit starts a new step.
func (c *Core) BreakCoalescing() { c.lastKind = EditOther }

// Undo restores the previous snapshot.
func (c *Core) Undo() bool {
	if len(c.undo) == 0 {
		return false
	}
	last := c.undo[len(c.undo)-1]
	c.undo = c.undo[:len(c.undo)-1]
	c.redo = append(c.redo, snapshot{doc: *c.Doc.Clone(), sel: c.Sel})
	c.Doc = last.doc.Clone()
	c.Sel = last.sel
	c.lastKind = EditOther
	c.touch()
	return true
}

// Redo reapplies the last undone snapshot.
func (c *Core) Redo() bool {
	if len(c.redo) == 0 {
		return false
	}
	last := c.redo[len(c.redo)-1]
	c.redo = c.redo[:len(c.redo)-1]
	c.undo = append(c.undo, snapshot{doc: *c.Doc.Clone(), sel: c.Sel})
	c.Doc = last.doc.Clone()
	c.Sel = last.sel
	c.lastKind = EditOther
	c.touch()
	return true
}

// Ordered returns the selection's start and end in document order.
func (c *Core) Ordered() (Pos, Pos) {
	a, b := c.Sel.Anchor, c.Sel.Head
	us := c.Units()
	ia, ib := -1, -1
	for i, u := range us {
		if u.ID == a.Block {
			ia = i
		}
		if u.ID == b.Block {
			ib = i
		}
	}
	if ia > ib || (ia == ib && a.Offset > b.Offset) {
		return b, a
	}
	return a, b
}

// SelectedText is the plain text of the selection.
func (c *Core) SelectedText() string {
	if c.Sel.Collapsed() {
		return ""
	}
	from, to := c.Ordered()
	if from.Block == to.Block {
		t := c.TextOf(from.Block)
		if t == nil {
			return ""
		}
		part := t.Slice(from.Offset, to.Offset)
		return part.PlainText()
	}
	var b strings.Builder
	if t := c.TextOf(from.Block); t != nil {
		part := t.Slice(from.Offset, t.Len())
		b.WriteString(part.PlainText())
	}
	us := c.Units()
	seen := false
	for _, u := range us {
		if u.ID == from.Block {
			seen = true
			continue
		}
		if !seen || !u.Text {
			continue
		}
		if u.ID == to.Block {
			break
		}
		if t := c.TextOf(u.ID); t != nil {
			b.WriteString("\n")
			b.WriteString(t.PlainText())
		}
	}
	b.WriteString("\n")
	if t := c.TextOf(to.Block); t != nil {
		part := t.Slice(0, to.Offset)
		b.WriteString(part.PlainText())
	}
	return b.String()
}

// InsertText types at the caret.
func (c *Core) InsertText(s string) {
	if s == "" {
		return
	}
	c.Checkpoint(EditInsert)
	c.DeleteSelection()
	p := c.Sel.Head
	t := c.TextOf(p.Block)
	if t == nil {
		return
	}
	m := t.MarksAt(p.Offset)
	if c.PendingMk != nil {
		m = *c.PendingMk
		c.PendingMk = nil
	}
	t.InsertPlain(p.Offset, s, m)
	c.Sel = Caret(Pos{Block: p.Block, Offset: p.Offset + len(s)})
	c.touch()
}

// ReplaceInBlock swaps a byte range of one block's text.
func (c *Core) ReplaceInBlock(id doc.BlockID, start, end int, s string) {
	t := c.TextOf(id)
	if t == nil {
		return
	}
	t.Replace(start, end, doc.PlainRich(s))
	c.touch()
}

// DeleteSelection removes whatever is selected.
func (c *Core) DeleteSelection() {
	if c.Sel.Collapsed() {
		return
	}
	from, to := c.Ordered()
	if from.Block == to.Block {
		if t := c.TextOf(from.Block); t != nil {
			t.Replace(from.Offset, to.Offset, doc.RichText{})
		}
		c.Sel = Caret(from)
		c.touch()
		return
	}
	fromText := c.TextOf(from.Block)
	toText := c.TextOf(to.Block)
	if fromText != nil && toText != nil {
		// Keep the head's prefix and the tail's suffix in one block.
		suffix := toText.Slice(to.Offset, toText.Len())
		*fromText = fromText.Slice(0, from.Offset)
		fromText.Append(suffix)
	}
	// Remove every block the selection covers beyond the first, plus the
	// units in between.
	us := c.Units()
	started := false
	var kill []doc.BlockID
	for _, u := range us {
		if u.ID == from.Block {
			started = true
			continue
		}
		if !started {
			continue
		}
		kill = append(kill, u.ID)
		if u.ID == to.Block {
			break
		}
	}
	for _, id := range kill {
		c.RemoveSubtree(id)
	}
	c.Sel = Caret(from)
	c.touch()
}

// Enter splits the block at the caret.
func (c *Core) Enter() {
	c.Checkpoint(EditOther)
	c.BreakCoalescing()
	c.DeleteSelection()
	p := c.Sel.Head
	if p.Block == Title {
		tail := c.Doc.Title.Slice(p.Offset, c.Doc.Title.Len())
		c.Doc.Title = c.Doc.Title.Slice(0, p.Offset)
		if tail.Text == "" && len(c.Doc.Blocks) > 0 {
			if _, ok := c.Doc.Blocks[0].Kind.(doc.TextKind); ok && c.Doc.Blocks[0].Text.Text == "" {
				c.Sel = Caret(Pos{Block: c.Doc.Blocks[0].ID})
				c.touch()
				return
			}
		}
		nb := doc.NewBlock(doc.TextKind{}, tail)
		c.Doc.Blocks = append([]*doc.Block{nb}, c.Doc.Blocks...)
		c.Sel = Caret(Pos{Block: nb.ID})
		c.touch()
		return
	}
	block := c.Block(p.Block)
	if block == nil || !block.Kind.HasText() {
		nb := doc.TextBlock("")
		c.InsertAfter(p.Block, nb)
		c.Sel = Caret(Pos{Block: nb.ID})
		c.touch()
		return
	}
	if ck, ok := block.Kind.(doc.CodeKind); ok {
		before := block.Text.Text[:p.Offset]
		line := before
		if i := strings.LastIndexByte(before, '\n'); i >= 0 {
			line = before[i+1:]
		}
		indent := ""
		for _, r := range line {
			if r != ' ' && r != '\t' {
				break
			}
			indent += string(r)
		}
		ins := "\n" + indent
		block.Text.InsertPlain(p.Offset, ins, doc.Marks{})
		_ = ck
		c.Sel = Caret(Pos{Block: p.Block, Offset: p.Offset + len(ins)})
		c.touch()
		return
	}
	empty := block.Text.IsEmpty()
	if empty && block.Kind.IsList() {
		if c.parentHasText(p.Block) {
			c.OutdentOne(p.Block)
		} else {
			block.Kind = doc.TextKind{}
		}
		c.Sel = Caret(Pos{Block: p.Block})
		c.touch()
		return
	}
	// Enter on an empty line leaves a blockquote, dropping the quote when
	// nothing is left in it, as Typora does.
	if empty {
		if parent := c.parentOf(p.Block); parent != nil {
			if _, isQuote := parent.Kind.(doc.QuoteKind); isQuote {
				nb := doc.NewBlock(doc.TextKind{}, doc.RichText{})
				c.InsertAfter(parent.ID, nb)
				c.RemoveSubtree(p.Block)
				if len(parent.Children) == 0 {
					c.RemoveSubtree(parent.ID)
				}
				c.Sel = Caret(Pos{Block: nb.ID})
				c.touch()
				return
			}
		}
	}
	// The block Enter starts continues the list a split sits in, and always
	// falls back to a paragraph after a heading, as Typora does.
	nextKind := block.Kind
	switch block.Kind.(type) {
	case doc.HeadingKind:
		nextKind = doc.TextKind{}
	case doc.OrderedKind:
		nextKind = doc.OrderedKind{}
	case doc.TodoKind:
		nextKind = doc.TodoKind{}
	case doc.BulletKind:
		nextKind = doc.BulletKind{}
	}
	if p.Offset == 0 && !empty {
		nb := doc.NewBlock(nextKind, doc.RichText{})
		nb.Align = block.Align
		c.InsertBefore(p.Block, nb)
		c.Sel = Caret(Pos{Block: p.Block})
		c.touch()
		return
	}
	tail := block.Text.Slice(p.Offset, block.Text.Len())
	block.Text = block.Text.Slice(0, p.Offset)
	nb := doc.NewBlock(nextKind, tail)
	nb.Align = block.Align
	if !nb.Text.IsEmpty() {
		nb.BG = block.BG
	}
	if len(block.Children) > 0 && !block.Folded {
		block.Children = append([]*doc.Block{nb}, block.Children...)
	} else {
		c.InsertAfter(c.FoldEnd(p.Block), nb)
	}
	c.Sel = Caret(Pos{Block: nb.ID})
	c.touch()
}

func resetForSplit(k doc.BlockKind) doc.BlockKind {
	switch kk := k.(type) {
	case doc.HeadingKind:
		return doc.HeadingKind{Level: kk.Level}
	case doc.OrderedKind:
		return doc.OrderedKind{}
	case doc.TodoKind:
		return doc.TodoKind{}
	}
	return k
}

func (c *Core) parentHasText(id doc.BlockID) bool {
	p, ok := c.Path(id)
	if !ok || len(p) < 2 {
		return false
	}
	parent := BlockAt(c.Doc.Blocks, p[:len(p)-1])
	return parent != nil && parent.Kind.HasText()
}

func (c *Core) parentOf(id doc.BlockID) *doc.Block {
	p, ok := c.Path(id)
	if !ok || len(p) < 2 {
		return nil
	}
	return BlockAt(c.Doc.Blocks, p[:len(p)-1])
}

// Backspace deletes backwards, merging blocks at the start of a line.
func (c *Core) Backspace() {
	if !c.Sel.Collapsed() {
		c.Checkpoint(EditDelete)
		c.DeleteSelection()
		c.touch()
		return
	}
	p := c.Sel.Head
	if p.Offset > 0 {
		c.Checkpoint(EditDelete)
		t := c.TextOf(p.Block)
		if t == nil {
			return
		}
		start := PrevBoundary(t.Text, p.Offset)
		t.Replace(start, p.Offset, doc.RichText{})
		c.Sel = Caret(Pos{Block: p.Block, Offset: start})
		c.touch()
		return
	}
	if p.Block == Title {
		return
	}
	block := c.Block(p.Block)
	c.Checkpoint(EditOther)
	c.BreakCoalescing()
	if block == nil || !block.Kind.HasText() {
		c.RemoveSubtree(p.Block)
		c.touch()
		return
	}
	switch block.Kind.(type) {
	case doc.TextKind:
		// fall through to merging
	case doc.CodeKind:
		if block.Text.IsEmpty() {
			block.Kind = doc.TextKind{}
			c.touch()
		}
		return
	default:
		block.Kind = doc.TextKind{}
		c.touch()
		return
	}
	if c.parentHasText(p.Block) {
		c.OutdentOne(p.Block)
		return
	}
	c.MergeIntoPrevious(p.Block)
}

// DeleteForward deletes the rune after the caret.
func (c *Core) DeleteForward() {
	if !c.Sel.Collapsed() {
		c.Checkpoint(EditDelete)
		c.DeleteSelection()
		c.touch()
		return
	}
	p := c.Sel.Head
	t := c.TextOf(p.Block)
	if t == nil {
		return
	}
	if p.Offset < t.Len() {
		c.Checkpoint(EditDelete)
		end := NextBoundary(t.Text, p.Offset)
		t.Replace(p.Offset, end, doc.RichText{})
		c.touch()
		return
	}
	// At the end: pull the next block's text up.
	us := c.Units()
	for i, u := range us {
		if u.ID != p.Block || i+1 >= len(us) {
			continue
		}
		next := us[i+1]
		if !next.Text {
			c.RemoveSubtree(next.ID)
			c.touch()
			return
		}
		nt := c.TextOf(next.ID)
		if nt == nil {
			return
		}
		c.Checkpoint(EditDelete)
		t.Append(*nt)
		c.RemoveSubtree(next.ID)
		c.touch()
		return
	}
}

// MergeIntoPrevious joins a block with the caret unit before it.
func (c *Core) MergeIntoPrevious(id doc.BlockID) {
	us := c.Units()
	idx := -1
	for i, u := range us {
		if u.ID == id {
			idx = i
			break
		}
	}
	if idx <= 0 {
		return
	}
	cur, prev := us[idx], us[idx-1]
	if prev.Scope != cur.Scope {
		return
	}
	if !prev.Text {
		c.RemoveSubtree(prev.ID)
		c.Sel = Caret(Pos{Block: id})
		c.touch()
		return
	}
	if _, isCode := c.Block(prev.ID).Kind.(doc.CodeKind); isCode {
		c.Sel = Caret(Pos{Block: prev.ID, Offset: c.LenOf(prev.ID)})
		return
	}
	text := doc.RichText{}
	if t := c.TextOf(id); t != nil {
		text = *t
	}
	block := c.Block(id)
	var kids []*doc.Block
	if block != nil {
		kids = block.Children
	}
	at := c.LenOf(prev.ID)
	if t := c.TextOf(prev.ID); t != nil {
		t.Append(text)
	}
	c.RemoveSubtree(id)
	if pb := c.Block(prev.ID); pb != nil && pb.Kind.Nests() {
		pb.Children = append(pb.Children, kids...)
	} else {
		for _, k := range kids {
			c.InsertAfter(prev.ID, k)
		}
	}
	c.Normalize()
	c.Sel = Caret(Pos{Block: prev.ID, Offset: at})
	c.touch()
}

// Normalize makes sure the document keeps a caret stop.
func (c *Core) Normalize() {
	if len(c.Doc.Blocks) == 0 {
		c.Doc.Blocks = []*doc.Block{doc.TextBlock("")}
	}
}

// Indent nests a text block under its previous sibling.
func (c *Core) Indent() {
	id := c.Sel.Head.Block
	p, ok := c.Path(id)
	if !ok || len(p) == 0 {
		return
	}
	last := p[len(p)-1]
	if last.IsCell || last.Index == 0 {
		return
	}
	list, idx := c.siblings(p)
	if list == nil {
		return
	}
	prev := (*list)[idx-1]
	if !prev.Kind.Nests() {
		return
	}
	c.Checkpoint(EditOther)
	c.detach(id)
	prev.Children = append(prev.Children, c.Block(id))
	c.touch()
}

// Outdent lifts a nested block to its parent's level.
func (c *Core) Outdent() {
	c.Checkpoint(EditOther)
	c.OutdentOne(c.Sel.Head.Block)
	c.touch()
}

// OutdentOne lifts one block one level.
func (c *Core) OutdentOne(id doc.BlockID) {
	p, ok := c.Path(id)
	if !ok || len(p) < 2 {
		return
	}
	list, idx := c.siblings(p)
	if list == nil || idx < 0 || idx >= len(*list) {
		return
	}
	b := (*list)[idx]
	*list = append((*list)[:idx], (*list)[idx+1:]...)
	parentPath := p[:len(p)-1]
	if len(parentPath) == 0 {
		return
	}
	glist, gidx := c.siblings(parentPath)
	if glist == nil || gidx < 0 || gidx >= len(*glist) {
		return
	}
	// Place the lifted block right after its former parent.
	*glist = append(*glist, nil)
	copy((*glist)[gidx+2:], (*glist)[gidx+1:])
	(*glist)[gidx+1] = b
}

func (c *Core) siblings(p Path) (*[]*doc.Block, int) {
	if len(p) == 0 {
		return nil, 0
	}
	last := p[len(p)-1]
	if last.IsCell {
		return nil, 0
	}
	if len(p) == 1 {
		return &c.Doc.Blocks, last.Index
	}
	parentPath := p[:len(p)-1]
	if parentPath[len(parentPath)-1].IsCell {
		cellSeg := parentPath[len(parentPath)-1]
		owner := BlockAt(c.Doc.Blocks, parentPath[:len(parentPath)-1])
		if owner == nil {
			return nil, 0
		}
		tk, ok := owner.Kind.(doc.TableKind)
		if !ok || tk.Table == nil {
			return nil, 0
		}
		if cellSeg.R >= len(tk.Table.Rows) || cellSeg.C >= len(tk.Table.Rows[cellSeg.R]) {
			return nil, 0
		}
		blocks := tk.Table.Rows[cellSeg.R][cellSeg.C].Blocks
		return &blocks, last.Index
	}
	parent := BlockAt(c.Doc.Blocks, parentPath)
	if parent == nil {
		return nil, 0
	}
	return &parent.Children, last.Index
}

// detach removes a block from its parent without deleting its subtree.
func (c *Core) detach(id doc.BlockID) *doc.Block {
	list, idx := c.siblingList(id)
	if list == nil || idx < 0 || idx >= len(*list) {
		return nil
	}
	b := (*list)[idx]
	*list = append((*list)[:idx], (*list)[idx+1:]...)
	return b
}

func (c *Core) siblingList(id doc.BlockID) (*[]*doc.Block, int) {
	p, ok := c.Path(id)
	if !ok {
		return nil, 0
	}
	return c.siblings(p)
}

// RemoveSubtree deletes a block and everything under it.
func (c *Core) RemoveSubtree(id doc.BlockID) *doc.Block {
	list, idx := c.siblingList(id)
	if list == nil || idx < 0 || idx >= len(*list) {
		return nil
	}
	b := (*list)[idx]
	*list = append((*list)[:idx], (*list)[idx+1:]...)
	c.Normalize()
	return b
}

// InsertAfter puts a block after a sibling.
func (c *Core) InsertAfter(id doc.BlockID, b *doc.Block) {
	list, idx := c.siblingList(id)
	if list == nil || idx < 0 || idx >= len(*list) {
		return
	}
	*list = append(*list, nil)
	copy((*list)[idx+2:], (*list)[idx+1:])
	(*list)[idx+1] = b
}

// InsertBefore puts a block before a sibling.
func (c *Core) InsertBefore(id doc.BlockID, b *doc.Block) {
	list, idx := c.siblingList(id)
	if list == nil || idx < 0 || idx >= len(*list) {
		return
	}
	*list = append(*list, nil)
	copy((*list)[idx+1:], (*list)[idx:])
	(*list)[idx] = b
}

// FoldEnd is the last block a folded heading's section hides.
func (c *Core) FoldEnd(id doc.BlockID) doc.BlockID {
	p, ok := c.Path(id)
	if !ok {
		return id
	}
	b := c.Block(id)
	if b == nil || !b.Folded {
		return id
	}
	list, idx := c.siblings(p)
	if list == nil {
		return id
	}
	level := uint8(0)
	if h, ok := b.Kind.(doc.HeadingKind); ok {
		level = h.Level
	}
	last := id
	for _, sib := range (*list)[idx+1:] {
		if h, ok := sib.Kind.(doc.HeadingKind); ok && h.Level <= level {
			break
		}
		last = sib.ID
	}
	return last
}

// ToggleMark turns a character format on or off.
func (c *Core) ToggleMark(get func(*doc.Marks) bool, set func(*doc.Marks, bool)) {
	from, to := c.Ordered()
	if c.Sel.Collapsed() {
		m := c.TextOf(from.Block)
		if m == nil {
			return
		}
		cur := m.MarksAt(from.Offset)
		on := !get(&cur)
		set(&cur, on)
		c.PendingMk = &cur
		return
	}
	c.Checkpoint(EditOther)
	allOn := true
	c.forEachSelectionSpan(from, to, func(t *doc.RichText, start, end int) {
		seg := t.Slice(start, end)
		ok := true
		for i := range seg.Runs {
			if !get(&seg.Runs[i].Marks) {
				ok = false
				break
			}
		}
		if !ok {
			allOn = false
		}
	})
	c.forEachSelectionSpan(from, to, func(t *doc.RichText, start, end int) {
		t.UpdateMarks(start, end, func(m *doc.Marks) { set(m, !allOn) })
	})
	c.touch()
}

// ApplyMarks rewrites marks over the selection.
func (c *Core) ApplyMarks(f func(*doc.Marks)) {
	from, to := c.Ordered()
	if c.Sel.Collapsed() {
		m := c.TextOf(from.Block)
		if m == nil {
			return
		}
		cur := m.MarksAt(from.Offset)
		f(&cur)
		c.PendingMk = &cur
		return
	}
	c.Checkpoint(EditOther)
	c.forEachSelectionSpan(from, to, func(t *doc.RichText, start, end int) {
		t.UpdateMarks(start, end, f)
	})
	c.touch()
}

// forEachSelectionSpan calls f for every (text, start, end) the selection
// covers.
func (c *Core) forEachSelectionSpan(from, to Pos, f func(*doc.RichText, int, int)) {
	if from.Block == to.Block {
		if t := c.TextOf(from.Block); t != nil {
			f(t, from.Offset, to.Offset)
		}
		return
	}
	if t := c.TextOf(from.Block); t != nil {
		f(t, from.Offset, t.Len())
	}
	us := c.Units()
	seen := false
	for _, u := range us {
		if u.ID == from.Block {
			seen = true
			continue
		}
		if !seen {
			continue
		}
		if u.ID == to.Block {
			break
		}
		if u.Text {
			if t := c.TextOf(u.ID); t != nil {
				f(t, 0, t.Len())
			}
		}
	}
	if t := c.TextOf(to.Block); t != nil {
		f(t, 0, to.Offset)
	}
}

// SetKind replaces the selected blocks' kind.
func (c *Core) SetKind(make func(existing doc.BlockKind) doc.BlockKind) {
	c.Checkpoint(EditOther)
	ids := c.selectedBlockIDs()
	if len(ids) == 0 {
		ids = []doc.BlockID{c.Sel.Head.Block}
	}
	for _, id := range ids {
		if b := c.Block(id); b != nil {
			b.Kind = make(b.Kind)
		}
	}
	c.touch()
}

// selectedBlockIDs lists the blocks the selection touches.
func (c *Core) selectedBlockIDs() []doc.BlockID {
	from, to := c.Ordered()
	var out []doc.BlockID
	us := c.Units()
	seen := false
	for _, u := range us {
		if u.ID == from.Block {
			seen = true
		}
		if !seen {
			continue
		}
		if u.Text {
			out = append(out, u.ID)
		}
		if u.ID == to.Block {
			break
		}
	}
	if len(out) == 0 {
		out = append(out, from.Block)
	}
	return out
}

// DuplicateBlocks copies the selected blocks below themselves.
func (c *Core) DuplicateBlocks(ids []doc.BlockID) (doc.BlockID, bool) {
	if len(ids) == 0 {
		ids = c.selectedBlockIDs()
	}
	if len(ids) == 0 {
		return 0, false
	}
	c.Checkpoint(EditOther)
	var last doc.BlockID
	for i := len(ids) - 1; i >= 0; i-- {
		b := c.Block(ids[i])
		if b == nil {
			continue
		}
		clone := b.FreshIDs()
		c.InsertAfter(b.ID, clone)
		last = clone.ID
	}
	c.touch()
	return last, last != 0
}

// DeleteBlock removes a block.
func (c *Core) DeleteBlock(id doc.BlockID) {
	c.Checkpoint(EditOther)
	c.RemoveSubtree(id)
	c.touch()
}

// InsertBlockAfter adds a block after the caret's block.
func (c *Core) InsertBlockAfter(id doc.BlockID, b *doc.Block) doc.BlockID {
	c.Checkpoint(EditOther)
	c.InsertAfter(id, b)
	c.touch()
	return b.ID
}

// MoveBlock moves a block before or after a target.
func (c *Core) MoveBlock(id, target doc.BlockID, after bool) bool {
	if id == target {
		return false
	}
	c.Checkpoint(EditOther)
	b := c.detach(id)
	if b == nil {
		return false
	}
	if after {
		c.InsertAfter(target, b)
	} else {
		c.InsertBefore(target, b)
	}
	c.touch()
	return true
}

// ToggleQuote wraps or unwraps a block in a quote.
func (c *Core) ToggleQuote() {
	id := c.Sel.Head.Block
	b := c.Block(id)
	if b == nil {
		return
	}
	c.Checkpoint(EditOther)
	if parent := c.parentOf(id); parent != nil {
		if _, ok := parent.Kind.(doc.QuoteKind); ok {
			kids := parent.Children
			idx := -1
			for i, k := range kids {
				if k.ID == id {
					idx = i
					break
				}
			}
			if idx >= 0 {
				c.Checkpoint(EditOther)
				parent.Children = append(kids[:idx], kids[idx+1:]...)
				for _, k := range kids[idx+1:] {
					_ = k
				}
				c.InsertAfter(parent.ID, b)
			}
			c.touch()
			return
		}
	}
	q := doc.NewBlock(doc.QuoteKind{}, doc.RichText{})
	c.InsertBefore(id, q)
	c.detach(id)
	q.Children = append(q.Children, b)
	c.Sel = Caret(Pos{Block: id})
	c.touch()
}

// SetHeading turns the block into a heading of the given level; level 0 turns
// it back into a paragraph.
func (c *Core) SetHeading(level int) {
	c.SetKind(func(doc.BlockKind) doc.BlockKind {
		if level <= 0 {
			return doc.TextKind{}
		}
		if level > 6 {
			level = 6
		}
		return doc.HeadingKind{Level: uint8(level)}
	})
}

// ToggleList switches between paragraph and bullet list.
func (c *Core) ToggleList() {
	c.SetKind(func(k doc.BlockKind) doc.BlockKind {
		if _, ok := k.(doc.BulletKind); ok {
			return doc.TextKind{}
		}
		return doc.BulletKind{}
	})
}

// ToggleOrdered switches between paragraph and numbered list.
func (c *Core) ToggleOrdered() {
	c.SetKind(func(k doc.BlockKind) doc.BlockKind {
		if _, ok := k.(doc.OrderedKind); ok {
			return doc.TextKind{}
		}
		return doc.OrderedKind{}
	})
}

// ToggleTodo switches between paragraph and task list.
func (c *Core) ToggleTodo() {
	c.SetKind(func(k doc.BlockKind) doc.BlockKind {
		if _, ok := k.(doc.TodoKind); ok {
			return doc.TextKind{}
		}
		return doc.TodoKind{}
	})
}

// ToggleCode switches between paragraph and code block.
func (c *Core) ToggleCode() {
	c.SetKind(func(k doc.BlockKind) doc.BlockKind {
		if _, ok := k.(doc.CodeKind); ok {
			return doc.TextKind{}
		}
		return doc.CodeKind{Lang: "plain text"}
	})
}

// ToggleTodoDone flips a task item's checkbox.
func (c *Core) ToggleTodoDone(id doc.BlockID) {
	b := c.Block(id)
	if b == nil {
		return
	}
	if t, ok := b.Kind.(doc.TodoKind); ok {
		c.Checkpoint(EditOther)
		b.Kind = doc.TodoKind{Done: !t.Done}
		c.touch()
	}
}

// MoveCaret steps one caret stop.
func (c *Core) MoveCaret(down bool, extend bool) {
	us := c.Units()
	idx := -1
	for i, u := range us {
		if u.ID == c.Sel.Head.Block {
			idx = i
			break
		}
	}
	if idx < 0 {
		return
	}
	next := idx
	if down {
		next = min(idx+1, len(us)-1)
	} else {
		next = max(idx-1, 0)
	}
	p := Pos{Block: us[next].ID}
	if !down {
		p.Offset = c.LenOf(p.Block)
	}
	c.setCaret(p, extend)
}

func (c *Core) setCaret(p Pos, extend bool) {
	if extend {
		c.Sel.Head = p
		return
	}
	c.Sel = Caret(p)
}

// OrderedNumber is the marker number for a list item at an index.
func OrderedNumber(siblings []*doc.Block, index int) uint32 {
	n := uint32(1)
	for i := 0; i < index && i < len(siblings); i++ {
		if o, ok := siblings[i].Kind.(doc.OrderedKind); ok {
			if o.Seq != nil {
				n = *o.Seq + 1
			} else {
				n++
			}
		}
	}
	return n
}
