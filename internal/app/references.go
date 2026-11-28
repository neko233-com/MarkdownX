package app

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type referenceIndex struct {
	document   *doc.Document
	generation uint64
	values     doc.References
}

type footnoteJump struct {
	definition doc.BlockID
	origin     editor.Pos
}

func (a *App) references() doc.References {
	if s := &a.referenceIndex; s.document != a.core.Doc || s.generation != a.core.Generation {
		*s = referenceIndex{a.core.Doc, a.core.Generation, a.core.Doc.References()}
	}
	return a.referenceIndex.values
}

func (a *App) referenceLabel(ref doc.ReferenceInline) (string, bool) {
	if ref.Footnote {
		if n, ok := a.references().Footnote(ref.Label); ok {
			return strconv.Itoa(n.Number), true
		}
	} else if _, ok := a.references().Link(ref.Label); ok {
		return ref.PlainText(), true
	}
	return ref.Markup, false
}

func (a *App) referenceAppearance(t doc.RichText) string {
	var labels strings.Builder
	for _, run := range t.Runs {
		if ref, ok := run.Marks.Inline.Reference(); ok {
			label, resolved := a.referenceLabel(ref)
			labels.WriteString(label)
			labels.WriteString(strconv.FormatBool(resolved))
			if ref.Image && resolved {
				link, _ := a.references().Link(ref.Label)
				labels.WriteByte('\x00')
				labels.WriteString(link.Destination)
			}
			labels.WriteByte('\x00')
		}
	}
	return labels.String()
}

func (a *App) referenceAttachment(in *doc.Inline, ref doc.ReferenceInline) *richtext.Attachment {
	label, resolved := a.referenceLabel(ref)
	if ref.Image && resolved {
		target, _ := a.references().Link(ref.Label)
		if img, err := a.md.LoadImage(target.Destination); err == nil && img != nil {
			w, h := ui.ImageSize(img)
			scale := min(float32(1), 180/max(w, 1), 76/max(h, 1))
			return &richtext.Attachment{Data: in, Name: label, URL: target.Destination, Image: img, Width: w * scale, Height: h * scale}
		}
	}
	color := a.inlineColor
	size := float32(16)
	if resolved {
		color = a.inlineAccent
		if ref.Footnote {
			size = 11
		}
	}
	font := ui.Font{Size: size}
	layout := ui.ShapeText(label, font, 0)
	w, h := layout.Size()
	height := h
	if ref.Footnote && resolved {
		height = max(h, 17)
	}
	return &richtext.Attachment{
		Data: in, Name: label, Width: max(1, w), Height: height,
		Draw: func(p *ui.Painter, box ui.Rect) {
			p.Clip(box, 0, func() {
				p.TextLayout(layout, box.X, box.Y, color)
				if resolved && !ref.Footnote {
					p.Fill(ui.Rect{X: box.X, Y: box.Y + box.H - 2, W: box.W, H: 1}, color, 0)
				}
			})
		},
	}
}

func referenceAtPosition(t doc.RichText, offset int) (doc.ReferenceInline, bool) {
	for _, span := range t.Spans() {
		if span.Start <= offset && offset < span.End {
			return span.Marks.Inline.Reference()
		}
	}
	return doc.ReferenceInline{}, false
}

func (a *App) referenceTooltip(e ui.Element, b *doc.Block, p *blockBuffer) {
	var text string
	if p.references != "" && a.inlineFor(b) == nil {
		x, y, over := e.PointerPosition()
		if over && e.Hovered() {
			if at, ok := p.inlineObjectAt(ui.Point{X: x, Y: y}); ok {
				text = a.referenceHint(b.Text, at)
			}
		}
	}
	if p.referenceTip != text {
		p.referenceTip = text
		// The tooltip timer reads anchors from the preceding frame.
		a.services.Invalidate()
	}
	e.Tooltip(text)
}

func (a *App) referenceHint(t doc.RichText, offset int) string {
	ref, ok := referenceAtPosition(t, offset)
	if !ok {
		return ""
	}
	if ref.Footnote {
		if n, ok := a.references().Footnote(ref.Label); ok {
			return (&doc.Document{Blocks: n.Body}).PlainText()
		}
	} else if link, ok := a.references().Link(ref.Label); ok {
		return firstNonEmpty(link.Title, link.Destination)
	}
	return ""
}

func (a *App) followReference(ref doc.ReferenceInline, origin editor.Pos) {
	if ref.Footnote {
		if n, ok := a.references().Footnote(ref.Label); ok {
			a.noteJump = footnoteJump{definition: n.Block, origin: origin}
			a.pendingNote = editor.Pos{Block: n.Block}
		}
	} else if link, ok := a.references().Link(ref.Label); ok {
		a.pendingLink = link.Destination
	}
}

func (a *App) definitionBlock(c *ui.Context, b *doc.Block) {
	a.blockInput(c, b, 14)
	if n := a.references().FootnoteDefinition(b.ID); n != nil && n.Number != 0 {
		link := ui.ButtonBase(c.Key("return-note")).Label("返回脚注引用").Cursor(ui.CursorPointer).Children(func() {
			ui.Text(c, "↩").TextColor(c.Theme().Accent)
		})
		if link.Clicked() {
			origin := editor.Pos{Block: n.OriginBlock, Offset: n.OriginOffset}
			if jump := a.noteJump; jump.definition == b.ID {
				if block := a.core.Block(jump.origin.Block); block != nil {
					if ref, ok := referenceAtPosition(block.Text, jump.origin.Offset); ok && ref.Footnote && strings.EqualFold(ref.Label, n.Label) {
						origin = jump.origin
					}
				}
			}
			if origin.Block == editor.Title {
				a.focusHeading(editor.Title)
			} else {
				a.pendingNote = origin
			}
			a.services.Invalidate()
		}
	}
	a.children(c, b)
}
