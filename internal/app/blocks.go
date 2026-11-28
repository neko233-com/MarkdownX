package app

import (
	"encoding/hex"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/richtext"

	"github.com/neko233/markdownx/internal/diagram"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type blockBuffer struct {
	native       *richtext.Document
	version      uint64
	model        doc.RichText
	visible      string
	align        doc.Align
	color        ui.Color
	accent       ui.Color
	references   string
	referenceTip string
	plainTyping  bool
	followHead   bool
	font         ui.Font
	tableWidth   cellWidthMeasurement
	bounds       ui.Rect
	layouts      map[int]*paragraphLayout
	language     fenceLanguage
	syntax       codeSyntax
	pointerPoint ui.Point
	pointerMods  ui.Modifiers
	pointerDown  bool
}

func (a *App) nativeText(b *doc.Block) richtext.Text {
	var runs []richtext.Run
	for _, span := range b.Text.Spans() {
		m := span.Marks
		style := richtext.Attributes{
			Italic: m.Italic, Underline: m.Underline, Strikethrough: m.Strike,
			Code: m.Code, Link: deref(m.Link, ""),
			Color: documentColor(m.TextColor), Background: documentColor(m.BGColor),
		}
		if m.Bold {
			style.Weight = 700
		}
		if m.TextColor != nil || m.BGColor != nil {
			style.Tag = deref(m.TextColor, "") + "\x00" + deref(m.BGColor, "")
		}
		if in := m.Inline; in != nil {
			style.Attachment = a.inlineAttachment(in)
		}
		runs = append(runs, richtext.Run{Text: b.Text.Text[span.Start:span.End], Attributes: style})
	}
	align := ui.Start
	switch b.Align {
	case doc.AlignCenter:
		align = ui.Center
	case doc.AlignRight:
		align = ui.End
	}
	return richtext.TextOf(richtext.Paragraph{Style: richtext.ParagraphStyle{Align: align}, Runs: runs})
}

func documentColor(s *string) ui.Color {
	if s == nil {
		return ui.Color{}
	}
	value := strings.TrimPrefix(*s, "#")
	if len(value) == 6 || len(value) == 8 {
		if _, err := hex.DecodeString(value); err == nil {
			return ui.Hex(value)
		}
	}
	return ui.Color{}
}

func documentText(t richtext.Text) doc.RichText {
	var out doc.RichText
	for _, run := range t.Runs(0, t.Len()) {
		at := run.Attributes
		m := doc.Marks{Bold: at.Bold(), Italic: at.Italic, Underline: at.Underline,
			Strike: at.Strikethrough, Code: at.Code}
		if at.Link != "" {
			m.Link = &at.Link
		}
		if color, bg, ok := strings.Cut(at.Tag, "\x00"); ok {
			if color != "" {
				m.TextColor = &color
			}
			if bg != "" {
				m.BGColor = &bg
			}
		}
		if att := at.Attachment; att != nil {
			if in, ok := att.Data.(*doc.Inline); ok {
				m.Inline = in
			} else if att.URL != "" {
				in := doc.ImageInlineObj(doc.InlineImage{SrcKind: "src", Src: att.URL, Alt: att.Name})
				m.Inline = &in
			}
			if m.Inline == nil {
				out.Push(att.Name, m)
				continue
			}
		}
		out.Push(run.Text, m)
	}
	return out
}

func (a *App) updateBlockBuffer(b *doc.Block, p *blockBuffer) {
	references := a.referenceAppearance(b.Text)
	if p.align == b.Align && p.color == a.inlineColor && p.references == references &&
		(references == "" || p.accent == a.inlineAccent) && reflect.DeepEqual(p.model, b.Text) {
		return
	}
	an, at := p.native.Selection()
	if r := a.inlineFor(b); r != nil && !reflect.DeepEqual(p.model, b.Text) {
		// A model command replaced this text, so its old projection is obsolete.
		an = doc.ByteToRune(b.Text.Text, a.nativeOffset(b, p, an))
		at = doc.ByteToRune(b.Text.Text, a.nativeOffset(b, p, at))
		a.inlineEdit = nil
	}
	visible := a.viewText(b)
	p.native.SetText(a.nativeText(visible))
	p.native.SetSelection(an, at)
	p.visible = visible.Text.Text
	p.version, p.model = p.native.Version(), b.Text
	p.align = b.Align
	p.color = a.inlineColor
	p.accent, p.references = a.inlineAccent, references
}

func (a *App) syncBlockBuffer(b *doc.Block, p *blockBuffer) bool {
	if v := p.native.Version(); v == p.version {
		return false
	}
	if r := a.inlineFor(b); r != nil {
		a.syncInlineSource(b, p, r)
		return false
	}
	next := documentText(p.native.Text())
	next.PreserveInlineSources(b.Text)
	p.visible = next.Text
	p.version, p.model = p.native.Version(), next
	if b.ID == a.focus && a.core.Sel.AcrossBlocks() {
		if span, ok := a.selectedSpans()[b.ID]; ok {
			before, after := b.Text.Text[:span.Start], b.Text.Text[span.End:]
			if end := next.Len() - len(after); end >= len(before) &&
				strings.HasPrefix(next.Text, before) && strings.HasSuffix(next.Text, after) {
				a.tableRow = nil
				a.core.ReplaceSelection(next.Slice(len(before), end))
				a.ResetBuffers()
				a.focusAt(a.core.Sel)
				return false
			}
		}
		// A native edit outside the projected range keeps its local scope.
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID, Offset: a.caretOf(b)})
	}
	if reflect.DeepEqual(next, b.Text) {
		return false
	}
	a.tableRow = nil
	a.setCoreSelection(b)
	if a.syncWorkspaceTemplateField(b, p, next) {
		return false
	}
	a.core.SyncRichText(b.ID, next)
	a.updateBlockBuffer(b, p)
	a.updateDirty()
	return true
}

func (a *App) blockBuffer(b *doc.Block) *blockBuffer {
	p := a.bufs[b.ID]
	if p == nil {
		native := richtext.NewDocument(a.nativeText(b))
		p = &blockBuffer{native: native, version: native.Version(), model: b.Text, visible: b.Text.Text, align: b.Align, color: a.inlineColor}
		p.accent, p.references = a.inlineAccent, a.referenceAppearance(b.Text)
		a.bufs[b.ID] = p
	}
	return p
}

func (a *App) blockInput(c *ui.Context, b *doc.Block, size float32, bold ...bool) ui.Element {
	pane := a.documentTab
	p := a.blockBuffer(b)
	changed := a.syncBlockBuffer(b, p)
	a.updateBlockBuffer(b, p)
	if size == 0 {
		size = documentFontSize
	}
	font := ui.Font{Size: size, LineHeight: documentLeading}
	switch k := b.Kind.(type) {
	case doc.HeadingKind:
		font.Weight = 700
		_, font.LineHeight = headingMetrics(k.Level)
	case doc.CodeKind:
		font.Family = "monospace"
	case doc.DefinitionKind:
		font.Family = "monospace"
	}
	if len(bold) > 0 && bold[0] {
		font.Weight = 700
	}
	if font.Family == "" {
		font.Family = firstNonEmpty(documentFont(), c.Theme().Font)
	}
	if p.font != font || p.layouts == nil {
		p.font = font
		p.layouts = make(map[int]*paragraphLayout)
	}
	if sel, ok := a.pending[b.ID]; ok {
		p.native.SetSelection(sel[0], sel[1])
		delete(a.pending, b.ID)
	}
	if p.plainTyping {
		p.native.SetTypingAttributes(richtext.Attributes{})
		p.plainTyping = false
	}
	p.syntax.prepare(b, c.Theme())
	options := richtext.EditorOptions{
		Label: "正文", Font: font, NoMarkdown: true, NoScrollbar: true,
		Theme:        &richtext.Theme{Text: a.inlineColor, CodeSize: documentCodeScale, CodeBackground: c.Theme().Surface},
		SpansVersion: p.syntax.version,
		Spans: func(i int, style richtext.ParagraphStyle, spans []ui.Span) []ui.Span {
			for j := range spans {
				if spans[j].Radius > 0 && spans[j].Font == "monospace" {
					spans[j].PaddingX = inlineCodePadding
				}
			}
			spans = p.syntax.paint(i, spans)
			p.layouts[i] = &paragraphLayout{spans: spans, align: style.Align}
			return spans
		},
		OnKey: func(key ui.Key, mods ui.Modifiers) bool {
			if !a.activatePaneInput(pane) {
				return false
			}
			a.syncBlockBuffer(b, p)
			a.readSelection(b, p)
			if a.workspaceTemplateKey(key, mods) {
				return true
			}
			if a.inlineKey(b, key, mods) {
				return true
			}
			if a.editorShortcut(mods, key) {
				return true
			}
			if a.inlineFor(b) != nil {
				if key != ui.KeyA || mods != ui.Cmd {
					return false
				}
				a.closeInlineSource(true)
			}
			if a.blockSelectionKey(b, p, key, mods) {
				return true
			}
			return a.onBlockKey(key, mods, b)
		},
		OnCommand: func(name string) bool { return a.activatePaneInput(pane) && a.blockCommand(b, p, name) },
		OnLink: func(dest string) {
			if a.activatePaneInput(pane) {
				a.inlineLinkClick(b, p, dest)
			}
		},
	}
	if k, ok := b.Kind.(doc.CodeKind); ok && a.diagrams && diagram.Language(k.Lang) != "" {
		options.Label = "图表源码"
	}
	if _, inTable := a.core.TableCellAt(b.ID); inTable {
		options.Menu = func(m *ui.Menu) { a.tableMenu(m, b, p) }
	} else if _, code := b.Kind.(doc.CodeKind); code {
		options.Menu = func(m *ui.Menu) {
			m.EditItems()
			m.Separator()
			if m.Item("复制代码内容").Chosen() {
				a.syncBlockBuffer(b, p)
				a.services.WriteClipboard(b.Text.Text)
			}
		}
	}
	e := richtext.EditorBase(c.Key(b.ID), p.native, options).FocusRing(false)
	height := size * font.LineHeight
	if h, ok := p.native.ContentHeight(); ok {
		height = max(height, h)
	} else {
		p.native.VisibleParagraphs()
	}
	e.Height(height)
	p.bounds = e.Bounds()
	if (e.Focused() && a.focusReq == 0) || a.focusReq == b.ID || (changed && a.focus == b.ID) {
		a.readSelection(b, p)
		a.projectBlockSelection(b, p)
	}
	e.ObserveInput(func(ev ui.InputEvent) {
		if pane != a.documentTab {
			return
		}
		a.observeWorkspaceTemplateField(b, p, ev)
		a.observeInlinePointer(b, p, ev)
		a.observeBlockSelection(b, p, ev)
	})
	a.referenceTooltip(e, b, p)
	a.followBlockDrag(c, e, b, p)
	a.dropFilesInBlock(c, e, b, p)
	selectionColor := c.Theme().Selection
	focused := e.Focused() || a.focusReq == b.ID
	if !changed {
		a.maybeShowInlineSource(b, p, focused)
	}
	span, selected := a.selectedSpans()[b.ID]
	e.DrawOver(func(paint *ui.Painter, r ui.Rect) {
		p.bounds = r
		if selected && !focused {
			p.drawSelection(paint, r, selectionColor, span)
		}
	})
	if a.focusReq == b.ID {
		e.Focus(ui.FocusOptions{PreventScroll: a.split != nil || a.keepViewportFocus})
		a.keepViewportFocus = false
		a.focusReq = 0
	}
	if a.focusMode && a.focus != 0 && a.focus != b.ID {
		e.Opacity(0.42)
	}
	if a.reveal == b.ID || (a.typeMode && e.Focused() && a.lastType != a.core.Sel.Head) {
		align := ui.ScrollNearest
		if a.typeMode {
			align = ui.ScrollCenter
		}
		e.ScrollIntoView(ui.ScrollIntoViewOptions{Block: align})
		a.reveal = 0
		a.lastType = a.core.Sel.Head
	}
	if changed && a.core.Sel.Collapsed() {
		at := a.core.Sel.Head.Offset
		if !a.autoFormat(b, at) {
			a.autoInline(b, at)
		}
	}
	return e
}

func (a *App) readSelection(b *doc.Block, p *blockBuffer) {
	if a.focus != b.ID {
		a.core.BreakCoalescing()
		a.core.PendingMk = nil
	}
	a.focus = b.ID
	anchor, head := p.native.Selection()
	if selected := a.tableRow; selected != nil &&
		(selected.focus != b.ID || min(anchor, head) != 0 || max(anchor, head) != utf8.RuneCountInString(b.Text.Text)) {
		a.tableRow = nil
	}
	a.sel[b.ID] = [2]int{a.nativeOffset(b, p, anchor), a.nativeOffset(b, p, head)}
	if a.core.Sel.AcrossBlocks() {
		if p.followHead && a.core.Sel.Head.Block == b.ID {
			a.core.Sel.Head.Offset = a.sel[b.ID][1]
			p.followHead = false
		}
		return
	}
	p.followHead = false
	a.core.Sel = editor.Selection{
		Anchor: editor.Pos{Block: b.ID, Offset: a.sel[b.ID][0]},
		Head:   editor.Pos{Block: b.ID, Offset: a.sel[b.ID][1]},
	}
}

func (a *App) onBlockKey(key ui.Key, mods ui.Modifiers, b *doc.Block) bool {
	sel := a.core.Sel
	pos := sel.Head
	if mods&(ui.Cmd|ui.Ctrl|ui.Alt) != 0 {
		return false
	}
	switch key {
	case ui.KeyEscape:
		if k, ok := b.Kind.(doc.CodeKind); ok && a.diagrams && diagram.Language(k.Lang) != "" {
			a.closeDiagramSource(b.ID)
			return true
		}
		return false
	case ui.KeyEnter:
		switch {
		case mods&ui.Shift != 0:
			if _, definition := b.Kind.(doc.DefinitionKind); definition && strings.HasPrefix(strings.TrimLeft(b.Text.Text, " \t"), "[^") {
				a.core.InsertText("\n    ")
			} else {
				a.core.HardBreak()
			}
			a.bufs[b.ID].plainTyping = true
		case !sel.AcrossBlocks() && a.formatOnEnter(b):
			return true
		default:
			a.core.Enter()
		}
	case ui.KeyTab:
		delta := 1
		if mods&ui.Shift != 0 {
			delta = -1
		}
		if a.core.MoveTableCell(b.ID, delta) {
			break
		}
		if _, code := b.Kind.(doc.CodeKind); code && delta == 1 {
			a.core.InsertText("\t")
		} else if delta == -1 {
			a.core.Outdent()
		} else {
			a.core.Indent()
		}
	case ui.KeyBackspace:
		if !sel.AcrossBlocks() && (!sel.Collapsed() || pos.Offset > 0) {
			return false
		}
		a.core.Backspace()
	case ui.KeyDelete:
		if !sel.AcrossBlocks() && (!sel.Collapsed() || pos.Offset != b.Text.Len()) {
			return false
		}
		if _, code := b.Kind.(doc.CodeKind); code && !sel.AcrossBlocks() {
			return false
		}
		a.core.DeleteForward()
	default:
		return false
	}
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
	return true
}
