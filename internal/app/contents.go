package app

import (
	"strings"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func (a *App) insertContents() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	contents := doc.NewBlock(doc.TOCKind{Marker: "[toc]"}, doc.RichText{})
	if _, paragraph := b.Kind.(doc.TextKind); paragraph && b.Text.IsEmpty() && len(b.Children) == 0 {
		a.afterFormat(a.core.ReplaceBlock(b.ID, contents, true))
		return
	}
	root := b.ID
	if path, ok := a.core.Path(root); ok {
		root = a.core.Doc.Blocks[path[0].Index].ID
	}
	a.core.InsertBlocks(root, contents, doc.TextBlock(""))
	a.afterFormat(a.core.Sel.Head)
}

func (a *App) contents(c *ui.Context, b *doc.Block) {
	container := ui.Column(c.Key(b.ID)).Gap(3).Padding(5, 10)
	a.drawSelectedObject(c, container, b.ID)
	container.ContextMenu(func(m *ui.Menu) {
		if m.Item("编辑目录标记").Chosen() {
			a.editContentsSource(b)
		}
		if m.Item("删除目录").Chosen() {
			a.core.DeleteBlock(b.ID)
			a.ResetBuffers()
			a.focusAt(a.core.Sel)
		}
	})
	container.Children(func() { a.contentsLinks(c, a.core.Doc) })
}

func (a *App) editContentsSource(b *doc.Block) {
	a.toggleSource()
	if a.source == nil {
		return
	}
	text, ranges := a.visualSource()
	for _, r := range ranges {
		if r.Block != b.ID {
			continue
		}
		line := text[r.Start:r.End]
		trimmed := strings.TrimSpace(line)
		off := strings.Index(line, trimmed)
		a.source.edit.SetSelections(sourceeditor.Selection{Anchor: r.Start + off, Head: r.Start + off + len(trimmed)})
		return
	}
}

func (a *App) previewContents(c *ui.Context, d *parser.Document, i int) bool {
	if d.Block(i).Kind != parser.Paragraph || !doc.IsTOCMarker(d.Lines(i)) {
		return false
	}
	ui.Column(c).Gap(3).Padding(5, 10).Children(func() { a.contentsLinks(c, a.core.Doc) })
	return true
}

func (a *App) contentsLinks(c *ui.Context, d *doc.Document) {
	headings := d.Headings(markdown.Slug)
	if len(headings) == 0 {
		ui.Text(c, "目录中暂无标题").TextColor(c.Theme().TextMuted)
		return
	}
	level := uint8(6)
	for _, h := range headings {
		level = min(level, h.Level)
	}
	for _, h := range headings {
		label := firstNonEmpty(h.Text, "空标题")
		link := ui.ButtonBase(c.Key(h.Block)).Label("目录："+label).Cursor(ui.CursorPointer).
			Padding(3, 6, 3, 6+float32(h.Level-level)*18).Children(func() {
			ui.Text(c, label).TextColor(c.Theme().Accent)
		})
		if link.Clicked() {
			a.jumpHeading(d, h.Block)
			a.services.Invalidate()
		}
	}
}

func (a *App) focusHeading(id doc.BlockID) {
	if id == editor.Title {
		a.closeInlineSource(true)
		a.core.Sel = editor.Caret(editor.Pos{Block: editor.Title})
		a.focus, a.focusReq = editor.Title, 0
		a.revealTitle = true
		return
	}
	a.focusAt(editor.Caret(editor.Pos{Block: id}))
}
