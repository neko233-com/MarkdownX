package app

import (
	"math"
	"runtime"
	"strings"
	"time"
	"unicode/utf16"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type selectionDrag struct {
	origin     doc.BlockID
	anchor     editor.Pos
	ready      bool
	wide       bool
	scrolledAt time.Time
}

type paragraphLayout struct {
	spans []ui.Span
	align ui.Align
	width float32
	shape *ui.TextLayout
}

func (p *blockBuffer) layout(index int, width float32) *ui.TextLayout {
	para := p.layouts[index]
	if para == nil {
		return nil
	}
	width = max(1, width)
	if para.shape == nil || para.width != width {
		para.shape = ui.ShapeRichTextWith(para.spans, p.font, ui.TextShape{Width: width, Align: para.align})
		para.width = width
	}
	return para.shape
}

func utf16Index(s string, offset int) int {
	units := 0
	for _, r := range s[:offset] {
		units += utf16.RuneLen(r)
	}
	return units
}

func utf16Offset(s string, units int) int {
	for i, r := range s {
		if units <= 0 {
			return i
		}
		units -= utf16.RuneLen(r)
	}
	return len(s)
}

func (p *blockBuffer) positionAt(point ui.Point) int {
	text := p.visible
	boxes := p.native.VisibleParagraphs()
	if len(boxes) == 0 {
		return 0
	}
	if point.Y < boxes[0].Top {
		point.Y = boxes[0].Top
	}
	for i, box := range boxes {
		if point.Y >= box.Top+box.Height && i+1 < len(boxes) {
			continue
		}
		start := doc.RuneToByte(text, box.Start)
		if shape := p.layout(box.Index, p.bounds.W); shape != nil {
			point.Y = min(point.Y-box.Top, box.Height)
			return start + utf16Offset(text[start:], shape.IndexAt(point))
		}
		return start
	}
	return len(text)
}

func (p *blockBuffer) drawSelection(paint *ui.Painter, r ui.Rect, color ui.Color, span editor.Span) {
	text := p.visible
	for _, box := range p.native.VisibleParagraphs() {
		start := doc.RuneToByte(text, box.Start)
		if start > span.End {
			break
		}
		shape := p.layout(box.Index, r.W)
		if shape == nil {
			continue
		}
		end := len(text)
		if n := strings.IndexByte(text[start:], '\n'); n >= 0 {
			end = start + n
		}
		lo, hi := max(start, span.Start), min(end, span.End)
		if lo >= hi {
			continue
		}
		selection := ui.TextInputRange{Start: utf16Index(text[start:end], lo-start), End: utf16Index(text[start:end], hi-start)}
		for _, rect := range shape.SelectionRects(selection) {
			top := max(0, box.Top+rect.Y)
			bottom := min(r.H, box.Top+rect.Y+rect.H)
			if top < bottom {
				paint.Fill(ui.Rect{X: r.X + rect.X, Y: r.Y + top, W: rect.W, H: bottom - top}, color, 0)
			}
		}
	}
}

func (a *App) selectedSpans() map[doc.BlockID]editor.Span {
	s := a.core.Sel
	if !s.AcrossBlocks() {
		return nil
	}
	if a.rangeCore != a.core || a.rangeGen != a.core.Generation || a.rangeSel != s {
		a.rangeCore, a.rangeGen, a.rangeSel = a.core, a.core.Generation, s
		a.rangeSpans = a.core.SelectionSpans()
	}
	return a.rangeSpans
}

func (a *App) projectBlockSelection(b *doc.Block, p *blockBuffer) {
	if p == nil {
		return
	}
	span, ok := a.selectedSpans()[b.ID]
	if !ok {
		return
	}
	a.closeInlineSource(false)
	start, end := span.Start, span.End
	if head := a.core.Sel.Head; head.Block == b.ID && head.Offset == start {
		start, end = end, start
	}
	lo, hi := doc.ByteToRune(b.Text.Text, start), doc.ByteToRune(b.Text.Text, end)
	if anchor, head := p.native.Selection(); anchor != lo || head != hi {
		p.native.SetSelection(lo, hi)
	}
}

func (a *App) selectEditorContent(b *doc.Block, p *blockBuffer) {
	a.closeInlineSource(true)
	if _, code := b.Kind.(doc.CodeKind); code {
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
		p.native.SetSelection(0, p.native.Text().Len())
		a.readSelection(b, p)
		return
	}
	anchor, _ := a.documentEdge(false)
	a.core.Sel = editor.Selection{Anchor: anchor, Head: editor.Pos{Block: b.ID, Offset: b.Text.Len()}, All: true}
	a.tableRow = nil
	a.core.BreakCoalescing()
	a.core.PendingMk = nil
	a.projectBlockSelection(b, p)
}

func (a *App) drawSelectedObject(c *ui.Context, e ui.Element, id doc.BlockID) {
	color := c.Theme().Selection
	e.DrawOver(func(p *ui.Painter, box ui.Rect) {
		if _, ok := a.selectedSpans()[id]; ok {
			p.Fill(box, color, 0)
		}
	})
}

func (a *App) blockCommand(b *doc.Block, p *blockBuffer, name string) bool {
	a.syncBlockBuffer(b, p)
	a.readSelection(b, p)
	if a.inlineFor(b) != nil {
		if name == "copy" || name == "cut" || name == "paste" {
			return false
		}
		a.closeInlineSource(true)
	}
	switch name {
	case "undo":
		a.undo()
	case "redo":
		a.redo()
	case "selectAll":
		a.selectEditorContent(b, p)
	case "copy", "cut", "delete":
		if !a.core.Sel.AcrossBlocks() {
			lo, hi := a.selectionOf(b)
			if name == "delete" || lo == hi || !hasReference(b.Text.Slice(lo, hi)) {
				return false
			}
		}
		if name == "delete" {
			a.core.ReplaceSelection(doc.RichText{})
			a.ResetBuffers()
			a.focusAt(a.core.Sel)
		} else {
			a.copyDocumentRange(name == "cut")
		}
	case "pasteAndMatchStyle":
		a.pastePlain()
	case "paste":
		return a.pasteDocumentBlocks()
	default:
		return false
	}
	return true
}

func (a *App) observeBlockSelection(b *doc.Block, p *blockBuffer, ev ui.InputEvent) {
	if ev.Kind != ui.InputPointerDown {
		return
	}
	if ev.Button == 1 && a.core.Sel.AcrossBlocks() {
		span, covered := a.selectedSpans()[b.ID]
		at := p.positionAt(ui.Point{X: ev.X, Y: ev.Y})
		if covered && span.Start <= at && at <= span.End {
			a.projectBlockSelection(b, p)
		} else {
			a.core.Sel = editor.Caret(editor.Pos{Block: b.ID, Offset: at})
		}
	}
	if ev.Button != 0 {
		return
	}
	a.syncBlockBuffer(b, p)
	drag := &selectionDrag{origin: b.ID}
	if ev.Mods&ui.Shift != 0 && a.core.Sel.Head.Block != 0 {
		drag.anchor, drag.ready = a.core.Sel.Anchor, true
		if drag.anchor.Block != b.ID {
			at := p.positionAt(ui.Point{X: ev.X, Y: ev.Y})
			a.core.Sel = editor.Selection{Anchor: drag.anchor, Head: editor.Pos{Block: b.ID, Offset: at}}
			drag.wide = true
			a.tableRow = nil
		}
	} else {
		// The native editor places the new caret after this callback.
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	}
	a.drag = drag
}

func (a *App) followBlockDrag(c *ui.Context, e ui.Element, b *doc.Block, p *blockBuffer) {
	drag := a.drag
	if drag == nil || drag.origin != b.ID {
		return
	}
	x, y, _ := e.PointerPosition()
	button, pressed := e.PressedButton()
	pressed = pressed && button == 0
	if !pressed {
		a.drag = nil
	}
	if e.PressCanceled() {
		if drag.wide {
			a.focusAt(editor.Caret(drag.anchor))
		}
		return
	}
	if !drag.ready {
		anchor, _ := p.native.Selection()
		drag.anchor = editor.Pos{Block: b.ID, Offset: a.nativeOffset(b, p, anchor)}
		drag.ready = true
	}
	point := ui.Point{X: p.bounds.X + x, Y: p.bounds.Y + y}
	if viewport := a.editBounds; viewport.H > 0 {
		if pressed {
			a.scrollSelection(c, drag, point.Y)
		}
		point.Y = min(max(point.Y, viewport.Y), viewport.Y+viewport.H-1)
	}
	id, input := a.nearestTextInput(point)
	if input == nil || (id == b.ID && !drag.wide && drag.anchor.Block == b.ID) {
		return
	}
	local := ui.Point{X: point.X - input.bounds.X, Y: point.Y - input.bounds.Y}
	sel := editor.Selection{Anchor: drag.anchor, Head: editor.Pos{Block: id, Offset: a.modelOffset(id, input.positionAt(local))}}
	drag.wide = sel.AcrossBlocks()
	a.core.Sel = sel
	a.tableRow = nil
	a.projectBlockSelection(b, p)
	if !pressed {
		a.focusAt(sel)
		a.services.Invalidate()
	}
}

func (a *App) scrollSelection(c *ui.Context, drag *selectionDrag, y float32) {
	const edge = 28
	const interval = time.Second / 60
	box := a.editBounds
	inside := min(max(y, box.Y+edge), box.Y+box.H-edge)
	velocity := max(-1, min(1, (y-inside)/edge)) * 900
	if velocity == 0 || (velocity < 0 && a.editScroll.Y == 0) || (velocity > 0 && a.editScroll.Y >= a.editScroll.MaxY) {
		drag.scrolledAt = time.Time{}
		return
	}
	now := c.Now()
	if drag.scrolledAt.IsZero() {
		drag.scrolledAt = now.Add(-interval)
	}
	seconds := min(0.05, now.Sub(drag.scrolledAt).Seconds())
	a.editScroll.Y = min(a.editScroll.MaxY, max(0, a.editScroll.Y+velocity*float32(seconds)))
	drag.scrolledAt = now
	c.After(interval)
}

func (a *App) nearestTextInput(point ui.Point) (doc.BlockID, *blockBuffer) {
	var id doc.BlockID
	var best *blockBuffer
	dist := float32(math.MaxFloat32)
	for _, u := range a.core.Units() {
		p := a.bufs[u.ID]
		if p == nil || p.bounds.W <= 0 || p.bounds.H <= 0 {
			continue
		}
		r := p.bounds
		if r.Contains(point.X, point.Y) {
			return u.ID, p
		}
		x := point.X - max(r.X, min(point.X, r.X+r.W))
		y := point.Y - max(r.Y, min(point.Y, r.Y+r.H))
		if next := x*x + y*y; next < dist {
			id, best, dist = u.ID, p, next
		}
	}
	return id, best
}

func (a *App) blockSelectionKey(b *doc.Block, p *blockBuffer, key ui.Key, mods ui.Modifiers) bool {
	command := ui.Ctrl
	byWord := ui.Ctrl
	if runtime.GOOS == "darwin" {
		command = ui.Super
		byWord = ui.Alt
	}
	if mods == command {
		switch key {
		case ui.KeyA:
			return a.blockCommand(b, p, "selectAll")
		case ui.KeyC:
			return a.blockCommand(b, p, "copy")
		case ui.KeyX:
			return a.blockCommand(b, p, "cut")
		case ui.KeyV:
			return a.blockCommand(b, p, "paste")
		}
	}
	s := a.core.Sel
	shift := mods&ui.Shift != 0
	forward := key == ui.KeyRight || key == ui.KeyDown || key == ui.KeyEnd || key == ui.KeyPageDown
	horizontal := key == ui.KeyLeft || key == ui.KeyRight
	vertical := key == ui.KeyUp || key == ui.KeyDown
	lineEdge := key == ui.KeyHome || key == ui.KeyEnd
	if !horizontal && !vertical && !lineEdge && key != ui.KeyPageUp && key != ui.KeyPageDown {
		return false
	}
	if mods == command|ui.Shift && ((runtime.GOOS == "darwin" && vertical) || (runtime.GOOS != "darwin" && lineEdge)) {
		if end, ok := a.documentEdge(forward); ok {
			if s.All {
				s.Anchor, _ = a.documentEdge(false)
			}
			s.Head, s.All = end, false
			a.focusAt(s)
		}
		return true
	}
	if s.AcrossBlocks() && !shift {
		start, end := a.core.Ordered()
		edge := start
		if forward {
			edge = end
		}
		if s.All {
			edge, _ = a.documentEdge(forward)
		}
		a.focusAt(editor.Caret(edge))
		return true
	}
	if s.All {
		s.Anchor, _ = a.documentEdge(false)
		s.Head, _ = a.documentEdge(true)
		s.All = false
		if s.Head.Block != b.ID {
			a.focusAt(s)
			return true
		}
		a.core.Sel = s
	}
	movement := mods &^ ui.Shift
	if ((movement == 0 && (horizontal || vertical)) || (movement == byWord && horizontal)) && (s.Collapsed() || shift) {
		at := s.Head.Offset
		edge := (!forward && at == 0) || (forward && at == b.Text.Len())
		caret, haveCaret := p.native.CaretRect()
		if vertical && haveCaret {
			edge = (!forward && caret.Y <= 1) || (forward && caret.Y+caret.H >= p.bounds.H-1)
		}
		if edge {
			if other, ok := a.neighborText(b.ID, forward); ok {
				if vertical && haveCaret {
					if next := a.bufs[other.Block]; next != nil && next.bounds.W > 0 {
						y := float32(0)
						if !forward {
							y = next.bounds.H
						}
						other.Offset = next.positionAt(ui.Point{X: p.bounds.X + caret.X - next.bounds.X, Y: y})
					}
				}
				s.Head = other
				if !shift {
					s.Anchor = other
				}
				a.focusAt(s)
				return true
			}
		}
	}
	// Let the native widget handle graphemes, Bidi, and movement inside a
	// visual line. The next selection read keeps the document's anchor.
	p.followHead = shift && s.AcrossBlocks()
	return false
}

func (a *App) neighborText(id doc.BlockID, forward bool) (editor.Pos, bool) {
	units := a.core.Units()
	for i, u := range units {
		if u.ID != id {
			continue
		}
		step := 1
		if !forward {
			step = -1
		}
		for j := i + step; j > 0 && j < len(units); j += step {
			if !units[j].Text {
				continue
			}
			p := editor.Pos{Block: units[j].ID}
			if !forward {
				p.Offset = a.core.LenOf(p.Block)
			}
			return p, true
		}
		break
	}
	return editor.Pos{}, false
}
