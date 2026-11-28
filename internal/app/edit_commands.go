package app

import (
	"strings"

	sourceeditor "github.com/egoist/mygo/ui/editor"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func (a *App) editFocused(edit func(*doc.Block) bool) bool {
	b := a.focused()
	if b == nil {
		return false
	}
	a.syncAll()
	a.setCoreSelection(b)
	if !edit(b) {
		return false
	}
	a.tableRow = nil
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
	return true
}

func (a *App) changeIndent(out bool) {
	a.editFocused(func(*doc.Block) bool {
		if out {
			a.core.Outdent()
		} else {
			a.core.Indent()
		}
		return true
	})
}

func (a *App) toggleTaskStatus() {
	a.editFocused(func(b *doc.Block) bool {
		if _, ok := b.Kind.(doc.TodoKind); !ok {
			return false
		}
		a.core.ToggleTodoDone(b.ID)
		return true
	})
}

func (a *App) moveTableColumn(delta int) {
	a.changeTable(func(c *editor.Core, id doc.BlockID) bool {
		return c.MoveTableColumn(id, delta)
	})
}

func (a *App) moveParagraph(delta int) {
	a.editFocused(func(b *doc.Block) bool {
		a.core.MoveParagraph(b.ID, delta)
		return true
	})
}

func (a *App) selectWord(remove bool) {
	if a.source != nil {
		e := a.source.edit.Selection()
		if e.Empty() {
			e.Anchor, e.Head = editor.WordBounds(a.source.text(), e.Head)
		}
		if !e.Empty() {
			a.applySourceRange(e.Start(), e.End(), remove)
		}
		return
	}
	a.editFocused(func(*doc.Block) bool {
		if a.core.Sel.Collapsed() && !a.core.SelectWord() {
			return false
		}
		if remove {
			a.core.Backspace()
		}
		return true
	})
}

func (a *App) changeHeading(delta int) {
	a.editFocused(func(b *doc.Block) bool {
		level := 0
		if h, ok := b.Kind.(doc.HeadingKind); ok {
			level = int(h.Level)
		}
		next := min(6, max(0, level+delta))
		if next == level {
			return false
		}
		a.core.SetHeading(next)
		return true
	})
}

func (a *App) clearFormatting() {
	a.editFocused(func(b *doc.Block) bool {
		if _, code := b.Kind.(doc.CodeKind); code {
			if !a.core.Sel.AcrossBlocks() {
				return false
			}
		}
		a.core.ApplyMarks(func(m *doc.Marks) { *m = doc.Marks{Inline: m.Inline} })
		return true
	})
}

func (a *App) jumpDocumentEdge(end bool) {
	if a.source != nil {
		at := 0
		if end {
			at = len(a.source.text())
		}
		a.source.edit.SetSelections(sourceeditor.Selection{Anchor: at, Head: at})
		a.source.focus = true
		return
	}
	if p, ok := a.documentEdge(end); ok {
		a.focusAt(editor.Caret(p))
	}
}

func (a *App) documentEdge(end bool) (editor.Pos, bool) {
	units := a.core.Units()
	for i := 1; i < len(units); i++ {
		at := i
		if end {
			at = len(units) - i
		}
		u := units[at]
		if u.Text {
			p := editor.Pos{Block: u.ID}
			if end {
				p.Offset = a.core.LenOf(u.ID)
			}
			return p, true
		}
	}
	return editor.Pos{}, false
}

func (a *App) jumpSelection() {
	if a.source != nil {
		a.source.edit.SetSelections(a.source.edit.Selections()...)
		a.source.focus = true
		return
	}
	if b := a.focused(); b != nil {
		a.setCoreSelection(b)
		a.focusAt(a.core.Sel)
	}
}

func (a *App) copyMarkdown() {
	if a.source != nil {
		s := a.source.text()
		sel := a.source.edit.Selection()
		from, to := sel.Start(), sel.End()
		if sel.Empty() {
			from = strings.LastIndexByte(s[:from], '\n') + 1
			if e := strings.IndexByte(s[to:], '\n'); e >= 0 {
				to += e
			} else {
				to = len(s)
			}
		}
		a.services.WriteClipboard(s[from:to])
		return
	}
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	if a.copyInlineSelection(b) {
		return
	}
	if a.core.Sel.AcrossBlocks() {
		text, _ := a.core.SelectedDocument().WorkspaceMarkdown()
		a.services.WriteClipboard(text)
		return
	}
	if row := a.tableRow; row != nil && row.focus == b.ID {
		if p, ok := a.core.TableCellAt(b.ID); ok && p.Block == row.table && row.row < len(p.Table.Rows) {
			a.services.WriteClipboard(doc.MarkdownTableRow(p.Table, row.row))
			return
		}
	}
	start, end := a.selectionOf(b)
	if start != end {
		a.services.WriteClipboard(doc.MarkdownInline(b.Text.Slice(start, end)))
		return
	}
	text, ranges := a.visualSource()
	for _, r := range ranges {
		if r.Block == b.ID {
			a.services.WriteClipboard(text[r.Start:r.End])
			return
		}
	}
	a.services.WriteClipboard(doc.MarkdownInline(b.Text))
}

func (a *App) pastePlain() {
	text := a.services.ReadClipboard()
	if text == "" {
		return
	}
	text = strings.NewReplacer("\r\n", "\n", "\r", "\n").Replace(text)
	if a.source != nil {
		a.source.edit.Insert(text)
		a.updateDirty()
		return
	}
	if a.pasteInlineSource(text) {
		return
	}
	a.editFocused(func(*doc.Block) bool {
		a.core.InsertPlainText(text)
		return true
	})
}

func (a *App) fullscreen() {
	if a.win != nil {
		a.win.SetFullScreen(!a.win.IsFullScreen())
	}
}

func (a *App) zoomBy(step float32) {
	a.zoom = min(1.75, max(0.75, a.zoom+step))
}
