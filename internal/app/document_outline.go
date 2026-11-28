package app

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type documentOutline struct {
	owner      *documentState
	model      *doc.Document
	generation uint64
	headings   []doc.Heading
	level      uint8
	list       ui.ListState
	cursor     int
	focus      bool
}

func (a *App) showDocumentOutline() {
	a.setSidebarPage(sidebarOutline)
	a.outline.focus = true
	a.remember()
}

func (a *App) documentOutlinePane(c *ui.Context) {
	view := &a.outline
	if view.owner != a.documentState {
		*view = documentOutline{owner: a.documentState, focus: view.focus, cursor: -1}
	}
	d := a.outlineDocument()
	if view.model != d || view.generation != a.core.Generation {
		view.model, view.generation = d, a.core.Generation
		view.headings = d.Headings(markdown.Slug)
		view.level = 6
		for _, h := range view.headings {
			view.level = min(view.level, h.Level)
		}
	}
	headings := view.headings
	t := c.Theme()
	if len(headings) == 0 {
		view.focus = false
		ui.Column(c.Key("empty-outline")).Grow(1).Fill().Padding(9, 8).Children(func() {
			ui.Text(c, "文档还没有标题").FontSize(13).TextColor(t.TextMuted)
		})
		return
	}

	current := a.activeOutlineRow(headings)
	label := func(i int) string { return firstNonEmpty(headings[i].Text, "空标题") }
	view.list.Key = func(i int) any { return headings[i].Block }
	view.list.Label = label
	view.list.Selected = &view.cursor
	list := ui.List(c.Key("document-outline"), &view.list, len(headings), func(i int) {
		h := headings[i]
		name := label(i)
		row := ui.Row(c).Height(26).MinWidth(0).FillWidth().AlignItems(ui.Center).
			Padding(0, 8, 0, 8+float32(h.Level-view.level)*10).Radius(t.Radius).Tooltip(name)
		if i == current && view.cursor != i {
			row.Background(t.Selection)
		}
		row.Children(func() {
			ui.Text(c, name).FontSize(13).SingleLine().MinWidth(0).TruncateMiddle()
		})
		if row.RowClicked() && view.owner == a.documentState {
			a.jumpHeading(d, h.Block)
			a.services.Invalidate()
		}
	}).Grow(1).Fill().Label("文档大纲")
	if view.focus {
		row := current
		if row < 0 {
			row = max(0, view.cursor)
		}
		view.list.Choose(min(row, len(headings)-1))
		view.list.ScrollIntoView(view.cursor)
		list.Focus()
		view.focus = false
	}
	if list.Submitted() && view.cursor >= 0 && view.cursor < len(headings) {
		a.jumpHeading(d, headings[view.cursor].Block)
		c.Invalidate()
	}
}

func (a *App) activeOutlineRow(headings []doc.Heading) int {
	if a.source != nil || a.mode == ModePreview || a.editBounds.H == 0 {
		return -1
	}
	top, bottom := a.editBounds.Y, a.editBounds.Y+a.editBounds.H
	y := top
	if caret := a.bufs[a.focus]; caret != nil && caret.bounds.H > 0 && caret.bounds.Y+caret.bounds.H > top && caret.bounds.Y < bottom {
		y = max(top, caret.bounds.Y)
	}
	current := -1
	for i, h := range headings {
		if h.Block == editor.Title {
			current = i
		} else if b := a.bufs[h.Block]; b != nil && b.bounds.H > 0 && b.bounds.Y <= y+1 {
			current = i
		}
	}
	return current
}
