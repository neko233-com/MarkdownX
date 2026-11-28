package app

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type imageSource struct {
	block          doc.BlockID
	initial, draft string
	focus, focused bool
}

func (a *App) editableImage(c *ui.Context, b *doc.Block) {
	if s := a.imageEdit; s != nil && s.block == b.ID {
		e := ui.TextArea(c.Key(b.ID), &s.draft).Lines(1, 5).Font("monospace").Label("图片 Markdown 源码")
		if s.focus {
			e.Focus()
			s.focus = false
		}
		e.OnTextKey(func(k ui.TextKey) bool {
			if k.Key == ui.KeyEscape && k.Mods == 0 {
				a.finishImageSource(false)
				return true
			}
			if k.Key == ui.KeyEnter && (k.Mods == 0 || k.Mods == ui.Cmd) {
				s.draft = k.Text()
				a.finishImageSource(true)
				return true
			}
			return false
		})
		if focused := e.Focused(); !focused && s.focused {
			a.finishImageSource(true)
		} else {
			s.focused = focused
		}
		return
	}
	d := &doc.Document{Blocks: []*doc.Block{b}}
	source, _ := d.WorkspaceMarkdown()
	e := ui.Column(c.Key(b.ID)).Role(ui.RoleButton).Focusable().Cursor(ui.CursorPointer).Label("编辑图片 Markdown 源码").Children(func() {
		markdown.View(c, source, &a.imageMD)
	})
	a.drawSelectedObject(c, e, b.ID)
	if e.Clicked() {
		a.closeInlineSource(true)
		a.focus = b.ID
		a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
		source = strings.TrimSuffix(source, "\n")
		a.imageEdit = &imageSource{block: b.ID, initial: source, draft: source, focus: true}
		a.services.Invalidate()
	}
}

func (a *App) finishImageSource(commit bool) {
	s := a.imageEdit
	if s == nil {
		return
	}
	a.imageEdit = nil
	a.services.Invalidate()
	if !commit || s.initial == s.draft || a.core.Block(s.block) == nil {
		return
	}
	parsed := doc.MarkdownToBlocks(s.draft, doc.Off{File: true})
	b := doc.NewBlock(doc.TextKind{}, doc.ParseInlineSource(s.draft))
	if len(parsed) == 1 {
		if _, image := parsed[0].Kind.(doc.ImageKind); image {
			b = parsed[0]
		}
	}
	a.core.ReplaceBlock(s.block, b, false)
	a.ResetBuffers()
}
