package app

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// A single span can show its Markdown in the native text control. Document
// positions and undo still use the parsed text, including while typing.
type inlineEditor struct {
	block   doc.BlockID
	span    doc.InlineSource
	visible string
	offsets []int
}

type inlineSourceRequest struct {
	block      doc.BlockID
	source     doc.InlineSource
	start, end int
}

func (r *inlineEditor) update(start int, source, visible string) {
	r.span = doc.InlineSource{Start: start, End: start + len(visible), Markup: source}
	r.visible, r.offsets = visible, doc.InlineSourceOffsets(source, visible)
}

func (r *inlineEditor) rawEnd() int { return r.span.Start + len(r.span.Markup) }

func (r *inlineEditor) modelOffset(at int) int {
	start := r.span.Start
	if at <= start {
		return at
	}
	if end := r.rawEnd(); at >= end {
		return r.span.End + at - end
	}
	i, match := slices.BinarySearch(r.offsets, at-start)
	if !match {
		i--
	}
	return start + doc.RuneToByte(r.visible, max(0, i))
}

func (r *inlineEditor) viewOffset(at int) int {
	start := r.span.Start
	if at < start {
		return at
	}
	if at > r.span.End {
		return r.rawEnd() + at - r.span.End
	}
	return start + r.offsets[doc.ByteToRune(r.visible, at-start)]
}

func (a *App) inlineFor(b *doc.Block) *inlineEditor {
	if a.inlineEdit != nil && a.inlineEdit.block == b.ID {
		return a.inlineEdit
	}
	return nil
}

func (a *App) viewText(b *doc.Block) *doc.Block {
	if r := a.inlineFor(b); r != nil {
		show := *b
		show.Text.Replace(r.span.Start, r.span.End, doc.PlainRich(r.span.Markup))
		return &show
	}
	return b
}

func (a *App) nativeOffset(b *doc.Block, p *blockBuffer, runeAt int) int {
	return a.modelOffset(b.ID, doc.RuneToByte(p.visible, runeAt))
}

func (a *App) modelOffset(id doc.BlockID, byteAt int) int {
	if r := a.inlineEdit; r != nil && r.block == id {
		return r.modelOffset(byteAt)
	}
	return byteAt
}

func (a *App) nativeRune(b *doc.Block, p *blockBuffer, byteAt int) int {
	if r := a.inlineFor(b); r != nil {
		byteAt = r.viewOffset(byteAt)
	}
	return doc.ByteToRune(p.visible, byteAt)
}

func (a *App) showInlineSource(b *doc.Block, p *blockBuffer, source doc.InlineSource, caret int) {
	r := &inlineEditor{block: b.ID}
	r.update(source.Start, source.Markup, b.Text.Text[source.Start:source.End])
	a.inlineEdit = r
	a.core.BreakCoalescing()
	a.core.PendingMk = nil
	show := a.viewText(b)
	p.native.SetText(a.nativeText(show))
	p.visible, p.version = show.Text.Text, p.native.Version()
	at := a.nativeRune(b, p, caret)
	if r.visible == doc.Obj {
		at = doc.ByteToRune(p.visible, r.span.Start+inlineObjectCaret(r.span.Markup))
	}
	p.native.SetSelection(at, at)
}

// Put image and equation editing inside their source, rather than beside the
// one glyph that represents the complete object in the document.
func inlineObjectCaret(source string) int {
	for _, prefix := range []string{"![", "[^", `\(`, "$$", "$"} {
		if strings.HasPrefix(source, prefix) {
			return len(prefix)
		}
	}
	return min(1, len(source))
}

func (a *App) syncInlineSource(b *doc.Block, p *blockBuffer, r *inlineEditor) {
	next := documentText(p.native.Text())
	p.version = p.native.Version()
	if next.Text == p.visible {
		return
	}
	from, to := r.span.Start, r.rawEnd()
	prefix, suffix := b.Text.Slice(0, r.span.Start), b.Text.Slice(r.span.End, b.Text.Len())
	if len(next.Text) >= prefix.Len()+suffix.Len() && strings.HasPrefix(next.Text, prefix.Text) && strings.HasSuffix(next.Text, suffix.Text) {
		to = next.Len() - suffix.Len()
	} else {
		start, oldEnd, newEnd := doc.InlineEditBounds(p.visible, next.Text)
		switch {
		case oldEnd <= from:
			from += newEnd - oldEnd
			to += newEnd - oldEnd
		case start < to:
			from = min(from, start)
			if oldEnd < to {
				to += newEnd - oldEnd
			} else {
				to = newEnd
			}
		}
		before, after := next.Slice(0, from), next.Slice(to, next.Len())
		before.PreserveInlineSources(prefix)
		after.PreserveInlineSources(suffix)
		prefix, suffix = before, after
	}
	source := next.Text[from:to]
	value := doc.ParseInlineSource(source)
	newText := prefix
	newText.Append(value)
	newText.Append(suffix)
	a.setCoreSelection(b)
	a.core.SyncRichText(b.ID, newText)
	r.update(from, source, value.Text)
	p.model, p.visible = b.Text, next.Text
	a.updateDirty()
}

func (a *App) closeInlineSource(suppress bool) {
	r := a.inlineEdit
	if r == nil {
		return
	}
	b, p := a.core.Block(r.block), a.bufs[r.block]
	if b == nil || p == nil {
		a.inlineEdit = nil
		return
	}
	if p.native.Version() != p.version {
		a.syncInlineSource(b, p, r)
	}
	anchor, caret := p.native.Selection()
	start, end := a.nativeOffset(b, p, anchor), a.nativeOffset(b, p, caret)
	a.inlineEdit = nil
	p.visible = b.Text.Text
	p.native.SetText(a.nativeText(b))
	p.native.SetSelection(doc.ByteToRune(p.visible, start), doc.ByteToRune(p.visible, end))
	p.model, p.version = b.Text, p.native.Version()
	a.core.BreakCoalescing()
	if a.focus == b.ID {
		a.readSelection(b, p)
	}
	if suppress {
		a.inlinePause = editor.Pos{Block: b.ID, Offset: end}
	}
}

func (a *App) maybeShowInlineSource(b *doc.Block, p *blockBuffer, focused bool) {
	if request := a.inlineRequest; focused && request != nil && request.block == b.ID {
		a.inlineRequest = nil
		a.showInlineSource(b, p, request.source, request.source.Start)
		p.native.SetSelection(doc.ByteToRune(p.visible, request.source.Start+request.start),
			doc.ByteToRune(p.visible, request.source.Start+request.end))
		return
	}
	if r := a.inlineFor(b); r != nil {
		// A tree press can take focus before the file drag starts. Keep the
		// authored span in place so its contents remain a drop target.
		if a.treePointerHeld {
			return
		}
		anchor, caret := p.native.Selection()
		lo, hi := doc.RuneToByte(p.visible, min(anchor, caret)), doc.RuneToByte(p.visible, max(anchor, caret))
		if !focused || lo < r.span.Start || hi > r.rawEnd() || a.core.Sel.AcrossBlocks() ||
			lo == hi && (lo == r.span.Start || hi == r.rawEnd()) {
			a.closeInlineSource(false)
		}
		return
	}
	if !focused || a.drag != nil || !a.core.Sel.Collapsed() {
		return
	}
	if _, code := b.Kind.(doc.CodeKind); code {
		return
	}
	pos := a.core.Sel.Head
	if pos.Block != b.ID {
		return
	}
	clicked := a.inlineClick.Block == b.ID
	if clicked {
		pos.Offset = a.inlineClick.Offset
		a.inlineClick = editor.Pos{}
	} else if a.inlinePause == pos {
		return
	}
	a.inlinePause = editor.Pos{}
	if _, display := displayEquation(b); display {
		return
	}
	if source, ok := b.Text.InlineSourceAt(pos.Offset, clicked); ok {
		a.closeInlineSource(false)
		a.showInlineSource(b, p, source, pos.Offset)
	}
}

func (a *App) inlineKey(b *doc.Block, key ui.Key, mods ui.Modifiers) bool {
	r := a.inlineFor(b)
	if r == nil {
		return false
	}
	switch key {
	case ui.KeyEscape:
		a.closeInlineSource(true)
		return true
	case ui.KeyTab:
		if mods == 0 && strings.HasPrefix(r.span.Markup, "[") {
			p := a.bufs[b.ID]
			_, head := p.native.Selection()
			off := doc.RuneToByte(p.visible, head) - r.span.Start
			if at := strings.Index(r.span.Markup, "]("); at >= 0 && off <= at {
				n := doc.ByteToRune(p.visible, r.span.Start+at+2)
				p.native.SetSelection(n, n)
				return true
			}
		}
		a.closeInlineSource(true)
	case ui.KeyEnter:
		a.closeInlineSource(true)
	}
	return false
}

func (a *App) copyInlineSelection(b *doc.Block) bool {
	r, p := a.inlineFor(b), a.bufs[b.ID]
	if r == nil || p == nil {
		return false
	}
	anchor, head := p.native.Selection()
	lo, hi := doc.RuneToByte(p.visible, min(anchor, head)), doc.RuneToByte(p.visible, max(anchor, head))
	if lo == hi || lo < r.span.Start || hi > r.rawEnd() {
		return false
	}
	a.services.WriteClipboard(p.visible[lo:hi])
	return true
}

func (a *App) pasteInlineSource(s string) bool {
	r := a.inlineEdit
	if r == nil {
		return false
	}
	b, p := a.core.Block(r.block), a.bufs[r.block]
	if b == nil || p == nil {
		return false
	}
	p.native.Insert(richtext.TextOf(richtext.Paragraph{Runs: []richtext.Run{{Text: s}}}))
	a.syncBlockBuffer(b, p)
	a.readSelection(b, p)
	return true
}
