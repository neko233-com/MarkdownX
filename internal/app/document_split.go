package app

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

type documentSplit struct {
	owner, active *documentTab
	panes         []*documentTab
	scroll        ui.ScrollState
	reveal        *documentTab
}

func (p *documentTab) tabOwner() *documentTab {
	if p.split != nil {
		return p.split.owner
	}
	return p
}

func (p *documentTab) activePane() *documentTab {
	if p.split != nil {
		return p.split.active
	}
	return p
}

func (p *documentTab) panes() []*documentTab {
	if p.split != nil {
		return p.split.panes
	}
	return []*documentTab{p}
}

func (a *App) documentPanes(yield func(*documentTab) bool) {
	for _, tab := range a.tabs {
		for _, pane := range tab.panes() {
			if !yield(pane) {
				return
			}
		}
	}
}

// openDocuments visits each model once, with the active editor first.
func (a *App) openDocuments(yield func(*documentTab) bool) {
	if len(a.tabs) == 0 {
		return
	}
	seen := map[*documentState]bool{a.documentState: true}
	if !yield(a.documentTab) {
		return
	}
	for pane := range a.documentPanes {
		if !seen[pane.documentState] {
			seen[pane.documentState] = true
			if !yield(pane) {
				return
			}
		}
	}
}

func (a *App) documentViews(state *documentState) func(func(*App, *documentTab) bool) {
	return func(yield func(*App, *documentTab) bool) {
		for _, window := range a.workspaceWindows() {
			for pane := range window.documentPanes {
				if pane.documentState == state && !yield(window, pane) {
					return
				}
			}
		}
	}
}

func (p *documentTab) rememberPaneSelection() {
	p.selection = p.core.Sel
	p.pendingMarks = p.core.PendingMk
	p.captureSelectionText()
}

func (p *documentTab) captureSelectionText() {
	p.selectionCore, p.selectionGen = p.core, p.core.Generation
	for i, pos := range [2]editor.Pos{p.selection.Anchor, p.selection.Head} {
		p.selectionText[i] = ""
		if text := p.core.TextOf(pos.Block); text != nil {
			p.selectionText[i] = text.Text
		}
	}
}

func (p *documentTab) restorePaneSelection() {
	p.refreshPaneSelection()
	p.core.Sel = p.selection
	p.core.PendingMk = p.pendingMarks
}

func (p *documentTab) claimSelection() {
	if prior := p.selectionOwner; prior != p {
		if prior != nil {
			prior.rememberPaneSelection()
		}
		p.restorePaneSelection()
		p.selectionOwner = p
	}
}

func (p *documentTab) refreshPaneSelection() {
	if p.selectionCore == p.core && p.selectionGen == p.core.Generation {
		return
	}
	selection := p.selection
	if !selection.All {
		anchor, okAnchor := p.currentPanePosition(selection.Anchor, p.selectionText[0])
		head, okHead := p.currentPanePosition(selection.Head, p.selectionText[1])
		switch {
		case okAnchor && okHead:
			selection = editor.Selection{Anchor: anchor, Head: head}
		case okHead:
			selection = editor.Caret(head)
		case okAnchor:
			selection = editor.Caret(anchor)
		default:
			selection = editor.Caret(editor.Pos{})
			for _, u := range editor.Units(p.core.Doc) {
				if u.ID != editor.Title && u.Text {
					selection = editor.Caret(editor.Pos{Block: u.ID})
					break
				}
			}
		}
	}
	p.selection = selection
	p.captureSelectionText()
	if p.focus != editor.Title && p.core.Block(p.focus) == nil {
		p.focus = selection.Head.Block
	}
	if !selection.AcrossBlocks() {
		if b := p.core.Block(selection.Head.Block); b != nil && b.Kind.HasText() {
			sel := [2]int{selection.Anchor.Offset, selection.Head.Offset}
			p.sel[b.ID] = sel
			p.pending[b.ID] = [2]int{doc.ByteToRune(b.Text.Text, sel[0]), doc.ByteToRune(b.Text.Text, sel[1])}
		}
	}
}

func (p *documentTab) currentPanePosition(pos editor.Pos, prior string) (editor.Pos, bool) {
	text := p.core.TextOf(pos.Block)
	if text == nil {
		return editor.Pos{}, false
	}
	if prior != text.Text {
		pos.Offset = rebasePaneOffset(prior, text.Text, pos.Offset)
	}
	pos.Offset = clampOffset(text.Text, pos.Offset)
	return pos, true
}

func rebasePaneOffset(before, after string, at int) int {
	start := 0
	for start < min(len(before), len(after)) && before[start] == after[start] {
		start++
	}
	if at <= start {
		return at
	}
	endOld, endNew := len(before), len(after)
	for endOld > start && endNew > start && before[endOld-1] == after[endNew-1] {
		endOld--
		endNew--
	}
	if at >= endOld {
		return endNew + at - endOld
	}
	return min(at, endNew)
}

func (a *App) focusPaneCaret() {
	a.keepViewportFocus = true
	if a.source != nil {
		a.source.focus = true
		return
	}
	pos := a.core.Sel.Head
	b := a.core.Block(pos.Block)
	if b == nil || !b.Kind.HasText() {
		b = a.core.Block(a.focus)
		if b == nil || !b.Kind.HasText() {
			if a.path == "" || *a.title != "" {
				a.revealTitle = true
				return
			}
			for _, unit := range editor.Units(a.core.Doc) {
				if unit.ID != editor.Title && unit.Text {
					a.focus, a.focusReq = unit.ID, unit.ID
					break
				}
			}
			return
		}
	}
	a.focus, a.focusReq = b.ID, b.ID
	if !a.core.Sel.AcrossBlocks() {
		sel := a.sel[b.ID]
		if pos.Block == b.ID {
			sel = [2]int{a.core.Sel.Anchor.Offset, pos.Offset}
		}
		a.restoreSelection(b.ID, doc.ByteToRune(b.Text.Text, sel[0]), doc.ByteToRune(b.Text.Text, sel[1]))
	}
}

// withDocumentPane projects one pane's selection while building or updating
// that editor. The document, undo history and disk state stay shared.
func (a *App) withDocumentPane(pane *documentTab, fn func()) {
	active := a.documentTab
	if pane == active {
		pane.claimSelection()
		fn()
		return
	}
	prior := pane.selectionOwner
	core := pane.core
	selection, marks := core.Sel, core.PendingMk
	a.documentTab = pane
	pane.claimSelection()
	defer func() {
		pane.rememberPaneSelection()
		if prior != nil {
			prior.restorePaneSelection()
		} else {
			core.Sel, core.PendingMk = selection, marks
		}
		pane.selectionOwner = prior
		a.documentTab = active
	}()
	fn()
}

func (a *App) cloneDocumentPane(pane *documentTab) *documentTab {
	if pane == a.documentTab {
		a.parkDocumentTab()
	}
	other := newDocumentPane(pane.documentState)
	other.selection = pane.selection
	other.selectionCore, other.selectionGen = pane.selectionCore, pane.selectionGen
	other.selectionText = pane.selectionText
	other.pendingMarks = pane.pendingMarks
	other.focus = pane.focus
	other.editScroll.Y, other.previewScroll.Y = pane.editScroll.Y, pane.previewScroll.Y
	other.ensureSourceView()
	if other.source != nil && pane.source != nil && pane.source.sourceDocument == other.sourceDocument {
		other.source.edit.SetSelections(pane.source.edit.Selections()...)
		other.source.edit.SetScrollY(pane.source.edit.ScrollY())
	}
	return other
}

func (a *App) splitRight() {
	a.insertSplitPane(a.cloneDocumentPane(a.documentTab), -1)
}

func (a *App) openDocumentInSplit(path string, index int) {
	path = absoluteDocumentPath(path)
	open := a.tabAt(path)
	if open != nil {
		if open.split != nil && open.split == a.split && open != a.documentTab {
			a.selectDocumentTab(open, true)
			return
		}
		a.insertSplitPane(a.cloneDocumentPane(open), index)
		return
	}
	pane, err := loadDocumentTab(path)
	if err != nil {
		a.status = "打开失败：" + err.Error()
		return
	}
	a.insertSplitPane(pane, index)
}

func (a *App) insertSplitPane(pane *documentTab, index int) {
	split := a.split
	if split == nil {
		split = &documentSplit{owner: a.documentTab, active: a.documentTab, panes: []*documentTab{a.documentTab}}
		a.split = split
	}
	if index < 0 {
		index = slices.Index(split.panes, split.active) + 1
	}
	pane.split = split
	split.panes = slices.Insert(split.panes, min(index, len(split.panes)), pane)
	a.selectDocumentTab(pane, true)
	a.services.Invalidate()
}

func (a *App) stepSplit(step int) {
	if a.split == nil {
		return
	}
	i := slices.Index(a.split.panes, a.documentTab) + step
	if i >= 0 && i < len(a.split.panes) {
		a.selectDocumentTab(a.split.panes[i], true)
	}
}

func (a *App) replaceDocumentPane(replacement *documentTab) {
	pane, i := a.documentTab, a.tabIndex()
	if split := pane.split; split != nil {
		replacement.split, replacement.splitWeight = split, pane.splitWeight
		split.panes[slices.Index(split.panes, pane)] = replacement
		split.active, split.reveal = replacement, replacement
		if split.owner == pane {
			split.owner = replacement
			a.tabs[i] = replacement
		}
		pane.split = nil
	} else {
		a.tabs[i] = replacement
	}
	a.documentTab = replacement
}

func (a *App) removeSplitPane(pane *documentTab) *documentTab {
	split := pane.split
	tabIndex := slices.Index(a.tabs, split.owner)
	i := slices.Index(split.panes, pane)
	split.panes = slices.Delete(split.panes, i, i+1)
	if split.active == pane {
		split.active = split.panes[min(i, len(split.panes)-1)]
	}
	if split.owner == pane || len(split.panes) == 1 {
		split.owner = split.panes[0]
		a.tabs[tabIndex] = split.owner
	}
	if len(split.panes) == 1 {
		split.owner.split, split.owner.splitWeight = nil, 1
	}
	pane.split, pane.splitWeight = nil, 1
	return split.active
}

func (a *App) hasOtherPane(pane *documentTab) bool {
	for _, other := range a.documentViews(pane.documentState) {
		if other != pane {
			return true
		}
	}
	return false
}

func (a *App) closeSplitPane(pane *documentTab) {
	if pane.split == nil {
		return
	}
	a.parkDocumentTab()
	previous := a.documentTab
	if !a.hasOtherPane(pane) {
		if pane.dirty && !a.confirmUnsaved(pane) {
			return
		}
		a.rememberClosedTab(pane, slices.Index(a.tabs, pane.tabOwner()))
	}
	keep := a.removeSplitPane(pane)
	if previous == pane {
		a.selectDocumentTab(keep, true)
	} else {
		a.selectDocumentTab(previous, false)
	}
	a.services.Invalidate()
}

func (a *App) detachSplitPane(pane *documentTab, index int) {
	if pane.split == nil {
		return
	}
	a.removeSplitPane(pane)
	if a.documentTab == pane {
		a.tabs = slices.Insert(a.tabs, min(index, len(a.tabs)), pane)
		a.tabReveal = true
		a.focusPaneCaret()
		a.services.Invalidate()
		return
	}
	a.insertDocumentTab(pane, min(index, len(a.tabs)), true)
	a.services.Invalidate()
}

func (p *documentTab) tabLabel() (string, bool) {
	if p.split == nil {
		return p.DisplayName(), p.dirty
	}
	names := make([]string, 0, len(p.split.panes))
	dirty := false
	for _, pane := range p.split.panes {
		names = append(names, pane.DisplayName())
		dirty = dirty || pane.dirty
	}
	return strings.Join(names, " | "), dirty
}
