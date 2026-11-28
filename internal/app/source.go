package app

import (
	"sort"
	"strings"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type sourceView struct {
	*sourceDocument
	edit  sourceeditor.State
	focus bool
}

type sourceDocument struct {
	buffer  *sourceeditor.Buffer
	exact   string
	version uint64
	parsed  *doc.Document
}

func newSourceView(source *sourceDocument) *sourceView {
	return &sourceView{sourceDocument: source, edit: sourceeditor.State{
		Buffer: source.buffer, Label: "Markdown 源码", SoftWrap: true,
		NoAutoClose: true, NoIndentGuides: true, NoFolding: true,
	}}
}

func (p *documentTab) ensureSourceView() {
	if p.sourceDocument == nil {
		p.source = nil
	} else if p.source == nil || p.source.sourceDocument != p.sourceDocument {
		p.source = newSourceView(p.sourceDocument)
	}
}

func (s *sourceDocument) text() string {
	if v := s.buffer.Version(); v != s.version {
		s.exact = s.buffer.String()
		s.version = v
		s.parsed = nil
	}
	return s.exact
}

func (s *sourceDocument) document() *doc.Document {
	text := s.text()
	if s.parsed == nil {
		s.parsed = doc.ReadSource(text)
	}
	return s.parsed
}

func (a *App) visualSource() (string, []doc.SourceRange) {
	if a.sourceGen != a.core.Generation || a.sourceDoc != a.core.Doc {
		a.sourceDoc = a.core.Doc
		a.sourceGen = a.core.Generation
		a.sourceText, a.sourceRanges = a.sourceDoc.WorkspaceMarkdown()
	}
	return a.sourceText, a.sourceRanges
}

func (a *App) markdownSource() string {
	if a.sourceDocument != nil {
		return a.sourceDocument.text()
	}
	text, _ := a.visualSource()
	return text
}

func (a *App) currentDocument() *doc.Document {
	a.syncAll()
	if a.source != nil {
		text, _ := a.visualSource()
		if src := a.source.text(); src != text {
			return a.source.document()
		}
	}
	return a.core.Doc
}

func (a *App) toggleSource() {
	a.endWorkspaceTemplateSession()
	if a.source != nil {
		a.leaveSource()
		return
	}
	a.syncAll()
	text, ranges := a.visualSource()
	a.enterSourceAt(text, a.visualSourceOffset(text, ranges))
}

func (a *App) visualSourceOffset(text string, ranges []doc.SourceRange) int {
	var at int
	if b := a.focused(); b != nil {
		if path, ok := a.core.Path(b.ID); ok {
			root := a.core.Doc.Blocks[path[0].Index].ID
			for _, r := range ranges {
				if r.Block == root {
					at = r.Start
					if edit := a.inlineFor(b); edit != nil {
						part := text[r.Start:r.End]
						prefix := doc.MarkdownInline(b.Text.Slice(0, edit.span.Start))
						markup := strings.Index(part, prefix+edit.span.Markup)
						if markup >= 0 {
							markup += len(prefix)
						} else {
							markup = strings.Index(part, edit.span.Markup)
						}
						if markup >= 0 {
							p := a.bufs[b.ID]
							_, head := p.native.Selection()
							off := doc.RuneToByte(p.visible, head) - edit.span.Start
							at += markup + min(len(edit.span.Markup), max(0, off))
						}
					} else if before := strings.Index(text[r.Start:r.End], b.Text.Text); before >= 0 {
						at += before + a.caretOf(b)
					}
					break
				}
			}
		}
	}
	return at
}

func (a *App) enterSourceAt(text string, at int) {
	a.closeInlineSource(true)
	a.closeFind()
	a.drag = nil
	buf := sourceeditor.NewBuffer(text)
	buf.SetLanguage(sourceeditor.LanguageNamed("Markdown"))
	a.sourceDocument = &sourceDocument{buffer: buf, exact: text, version: buf.Version()}
	a.source = newSourceView(a.sourceDocument)
	a.source.focus = true
	a.source.edit.Select(at, at)
	active := a.documentTab
	_, ranges := a.visualSource()
	for window, pane := range a.documentViews(active.documentState) {
		if pane == active {
			continue
		}
		window.withDocumentPane(pane, func() {
			offset := window.visualSourceOffset(text, ranges)
			window.closeInlineSource(true)
			window.closeFind()
			pane.source = newSourceView(a.sourceDocument)
			pane.source.edit.Select(offset, offset)
		})
		window.services.Invalidate()
	}
}

func (a *App) leaveSource() {
	s := a.source
	if s == nil {
		return
	}
	text := s.text()
	current, ranges := a.visualSource()
	caret := s.edit.Selection().Head
	d := a.core.Doc
	if text != current {
		d = s.document()
		_, ranges = d.WorkspaceMarkdown()
	}
	p := sourceCaret(d, text, ranges, caret)
	active := a.documentTab
	positions := make(map[*documentTab]editor.Pos)
	for _, pane := range a.documentViews(active.documentState) {
		if pane != active && pane.source != nil {
			positions[pane] = sourceCaret(d, text, ranges, pane.source.edit.Selection().Head)
		}
	}
	a.clearSharedSource()
	if d != a.core.Doc {
		a.core.ReplaceDocument(d, p)
	}
	a.ResetBuffers()
	if p.Block != editor.Title {
		a.focusAt(editor.Caret(p))
	}
	for pane, pos := range positions {
		pane.selection = editor.Caret(pos)
		pane.captureSelectionText()
		pane.focus = pos.Block
	}
	a.updateDirty()
	a.checkSavedText()
}

func (a *App) clearSharedSource() {
	a.sourceDocument = nil
	a.source = nil
	for window, pane := range a.documentViews(a.documentState) {
		pane.source = nil
		window.services.Invalidate()
	}
}

func sourceCaret(d *doc.Document, text string, ranges []doc.SourceRange, at int) editor.Pos {
	if len(ranges) == 0 {
		if len(d.Blocks) != 0 {
			return editor.Pos{Block: d.Blocks[0].ID}
		}
		return editor.Pos{}
	}
	i := sort.Search(len(ranges), func(i int) bool { return ranges[i].Start > at })
	r := ranges[max(i-1, 0)]
	path, _ := editor.FindPath(d.Blocks, r.Block)
	b := editor.BlockAt(d.Blocks, path)
	if b != nil && b.Kind.HasText() {
		offset := 0
		if n := strings.Index(text[r.Start:r.End], b.Text.Text); n >= 0 {
			offset = clampOffset(b.Text.Text, at-r.Start-n)
		}
		return editor.Pos{Block: r.Block, Offset: offset}
	}
	for _, u := range editor.Units(d) {
		if u.ID == 0 || !u.Text {
			continue
		}
		path, _ := editor.FindPath(d.Blocks, u.ID)
		if len(path) > 0 && d.Blocks[path[0].Index].ID == r.Block {
			return editor.Pos{Block: u.ID}
		}
	}
	return editor.Pos{}
}

func (a *App) sourcePane(c *ui.Context) {
	s := a.source
	pane := a.documentTab
	s.edit.OnInput = func(ev ui.InputEvent) bool {
		if session := a.templateSession; session != nil && session.source == s && a.workspaceTemplateSourceInput(s, ev) {
			return true
		}
		return ev.Kind == ui.InputKeyDown && a.activatePaneInput(pane) && a.editorShortcut(ev.Mods, ev.Key)
	}
	e := sourceeditor.View(c.Key("source"), &s.edit).Fill()
	if s.focus {
		e.Focus(ui.FocusOptions{PreventScroll: a.split != nil || a.keepViewportFocus})
		a.keepViewportFocus = false
		s.focus = false
	}
	refs, over := a.documentDrop(e)
	if over {
		x, y, _ := e.PointerPosition()
		if _, caret, ok := s.dropCaret(x, y); ok {
			color := c.Theme().Accent
			e.DrawOver(func(p *ui.Painter, r ui.Rect) {
				p.Fill(ui.Rect{X: r.X + caret.X, Y: r.Y + caret.Y, W: 2, H: caret.H}, color, 0)
			})
			showWorkspaceDropHint(c, "插入到此处")
		}
	}
	if len(refs) != 0 {
		a.pendingPane = a.documentTab
		x, y, _ := e.PointerPosition()
		if at, _, located := s.dropCaret(x, y); located {
			a.insertSourceDrop(e, refs, at)
		}
	}
	a.syncWorkspaceTemplateSource(s)
	a.updateDirty()
}

func (a *App) insertSourceDrop(e ui.Element, refs []workspaceReference, at int) {
	s := a.source
	a.endWorkspaceTemplateSession()
	syntax := sourceDropSyntax(s.text(), at)
	separator := " "
	if syntax == dropBlockPath {
		separator = "\n"
	} else if syntax == dropMarkdown && len(refs) > 1 {
		snapshot := s.edit.Buffer.Snapshot()
		if strings.TrimSpace(snapshot.Line(snapshot.LineOf(at))) == "" {
			separator = "\n\n"
		}
	}
	s.edit.Select(at, at)
	s.edit.Insert(documentDropText(refs, syntax != dropMarkdown, separator))
	e.Focus()
}
