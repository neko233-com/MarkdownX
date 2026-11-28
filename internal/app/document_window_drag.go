package app

import (
	"fmt"
	"slices"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/editor"
)

type documentDrag struct {
	from *App
	tab  *documentTab

	native, finished, accepted, delivered bool
	pending                               func()
}

// AppKit and OLE need a representation in addition to MyGo's local value.
// The marker carries no document names, paths, text or other editor state.
var nativeDocumentDragData = transfer.New(transfer.NewItem(transfer.Bytes("application/x-markdownx-document-drag", []byte{1})))

func (a *App) dragDocumentPart(element ui.Element, tab *documentTab, paneOnly bool) {
	drag := &documentDrag{from: a, tab: tab, native: a.win != nil && a.windows != nil}
	if paneOnly {
		element.Drag(splitPaneDrag{drag})
	} else {
		element.Drag(drag)
	}
	if drag.native {
		element.DragData(nativeDocumentDragData, transfer.DragOptions{
			Operations: transfer.Copy, // MyGo uses Copy to deliver its process-local value.
			Done:       func(result transfer.Result) { drag.finish(paneOnly, result) },
		})
	}
}

// The destination can render before or after the native tracking loop returns.
// Defer the move until that loop is done, especially when the source will close.
func (drag *documentDrag) onDrop(action func()) {
	if drag.delivered {
		return
	}
	if !drag.native || drag.finished && drag.accepted {
		drag.delivered = true
		action()
	} else if !drag.finished && drag.pending == nil {
		drag.pending = action
	}
}

func (drag *documentDrag) finish(paneOnly bool, result transfer.Result) {
	if drag.finished {
		return
	}
	drag.finished = true
	action := drag.pending
	drag.pending = nil
	if result.Err != nil {
		drag.from.status = "拖动未完成：" + result.Err.Error()
		drag.from.services.Invalidate()
		return
	}
	drag.accepted = result.Operation != transfer.None && !result.Canceled && !result.Escaped
	if drag.accepted {
		if action != nil {
			drag.onDrop(action)
		}
		return
	}
	if result.Operation == transfer.None && !result.Escaped && drag.from.windows != nil {
		point := mygo.Point{X: result.Point.X, Y: result.Point.Y}
		drag.from.windows.detachDocument(drag, paneOnly, &point)
	}
}

func (d *desktopWindows) ownsDocumentDrag(drag *documentDrag, paneOnly bool) bool {
	if drag == nil || drag.from == nil || drag.tab == nil || drag.from.windows != d || !slices.Contains(d.order, drag.from) {
		return false
	}
	if !paneOnly {
		return slices.Contains(drag.from.tabs, drag.tab)
	}
	split := drag.tab.split
	return split != nil && slices.Contains(drag.from.tabs, split.owner) && slices.Contains(split.panes, drag.tab)
}

func (a *App) receiveDocumentDrag(drag *documentDrag, paneOnly bool, index int) {
	if drag.from == a {
		if paneOnly {
			a.detachSplitPane(drag.tab, min(index, len(a.tabs)))
		} else {
			a.reorderDocumentTab(drag.tab, min(index, len(a.tabs)))
		}
		return
	}
	d := a.windows
	if d == nil || len(a.tabs) == 0 || !slices.Contains(d.order, a) || !d.ownsDocumentDrag(drag, paneOnly) {
		return
	}
	if d.moveDocument(drag, paneOnly, a, index) {
		d.focusTransferredWindow(a)
		d.closeTablessWindow(drag.from)
	}
}

func (a *App) receiveSplitPaneDrag(drag *documentDrag, split *documentSplit, index int) {
	if drag.from == a {
		a.reorderSplitPane(split, drag.tab, index)
		return
	}
	d := a.windows
	if d == nil || len(a.tabs) == 0 || a.split != split || !slices.Contains(d.order, a) || !d.ownsDocumentDrag(drag, true) {
		return
	}
	if d.prepareDocumentMove(drag, true, a) {
		a.insertSplitPane(drag.tab, index)
		d.focusTransferredWindow(a)
		d.closeTablessWindow(drag.from)
	}
}

func (d *desktopWindows) detachDocument(drag *documentDrag, paneOnly bool, point *mygo.Point) {
	if !d.ownsDocumentDrag(drag, paneOnly) || drag.from.win == nil {
		return
	}
	source := drag.from
	destination := New("")
	destination.windows = d
	destination.root = source.root
	destination.mode, destination.zoom = source.mode, source.zoom
	destination.hideSide, destination.sidebarWidth = source.hideSide, source.sidebarWidth
	destination.focusMode, destination.typeMode = source.focusMode, source.typeMode
	destination.autoSaveOn, destination.diagrams = source.autoSaveOn, source.diagrams
	destination.tabs = nil // The incoming tab is the initial document.
	if !d.moveDocument(drag, paneOnly, destination, 0) {
		return
	}
	destination.refreshFiles()
	destination.revealWorkspaceFile()
	d.showWindow(destination, point)
	d.closeTablessWindow(source)
}

// moveDocument transfers live editor state. The caller owns window creation,
// focus and closing, after both tab strips have committed the change.
func (d *desktopWindows) moveDocument(drag *documentDrag, paneOnly bool, destination *App, index int) bool {
	if !d.prepareDocumentMove(drag, paneOnly, destination) {
		return false
	}
	tab := drag.tab
	destination.tabs = slices.Insert(destination.tabs, min(max(index, 0), len(destination.tabs)), tab)
	destination.activateDocumentTab(tab.activePane(), true)
	destination.services.Invalidate()
	return true
}

func (d *desktopWindows) prepareDocumentMove(drag *documentDrag, paneOnly bool, destination *App) bool {
	source := drag.from
	tab := drag.tab
	panes := []*documentTab{tab}
	if !paneOnly {
		panes = tab.panes()
	}
	for _, pane := range panes {
		if existing := destination.tabAt(pane.path); existing != nil && existing.documentState != pane.documentState {
			status := fmt.Sprintf("目标窗口已独立打开 %s，标签页仍留在原窗口", pane.DisplayName())
			source.status, destination.status = status, status
			source.services.Invalidate()
			destination.services.Invalidate()
			return false
		}
	}
	source.parkDocumentTab()
	if len(destination.tabs) != 0 {
		destination.parkDocumentTab()
	}
	source.removeTransferredDocument(tab, paneOnly)
	for _, pane := range panes {
		pane.md, pane.imageMD = markdown.Options{}, markdown.Options{}
		pane.previewFolder = ""
		if source.pendingPane == pane {
			source.pendingPane = nil
		}
	}
	source.services.Invalidate()
	return true
}

func (a *App) removeTransferredDocument(tab *documentTab, paneOnly bool) {
	if paneOnly {
		next := a.removeSplitPane(tab)
		if a.documentTab == tab {
			a.activateDocumentTab(next, true)
		}
		return
	}
	index := slices.Index(a.tabs, tab)
	active := a.documentTab.tabOwner() == tab
	a.tabs = slices.Delete(a.tabs, index, index+1)
	if active && len(a.tabs) != 0 {
		a.activateDocumentTab(a.tabs[min(index, len(a.tabs)-1)].activePane(), true)
	}
}

func (d *desktopWindows) focusTransferredWindow(a *App) {
	d.active = a
	if a.win != nil {
		a.win.Focus()
	}
}

func (d *desktopWindows) closeTablessWindow(a *App) {
	if len(a.tabs) == 0 {
		if a.win != nil {
			a.win.Close()
		} else {
			d.remove(a)
		}
	}
}

func detachedWindowBounds(point mygo.Point, area mygo.Rectangle) mygo.Rectangle {
	bounds := mygo.Rectangle{X: point.X - 72, Y: point.Y - 16, Width: desktopWindowWidth, Height: desktopWindowHeight}
	if area.Width > 0 && area.Height > 0 {
		bounds.Width = min(bounds.Width, max(area.Width, desktopWindowMinWidth))
		bounds.Height = min(bounds.Height, max(area.Height, desktopWindowMinHeight))
		bounds.X = min(max(bounds.X, area.X), area.X+max(area.Width-bounds.Width, 0))
		bounds.Y = min(max(bounds.Y, area.Y), area.Y+max(area.Height-bounds.Height, 0))
	}
	return bounds
}

type documentRevision struct {
	core       *editor.Core
	generation uint64
	source     *sourceDocument
	version    uint64
	path       string
	dirty      bool
}

func (a *App) publishDocumentUpdates() {
	if a.windows == nil || len(a.windows.order) < 2 {
		return
	}
	for pane := range a.openDocuments {
		state := pane.documentState
		revision := documentRevision{core: state.core, generation: state.core.Generation, source: state.sourceDocument, path: state.path, dirty: state.dirty}
		if revision.source != nil {
			revision.version = revision.source.buffer.Version()
		}
		if state.published == revision {
			continue
		}
		state.published = revision
		var lastWindow *App
		for window := range a.documentViews(state) {
			if window != a && window != lastWindow {
				lastWindow = window
				window.services.Invalidate()
			}
		}
	}
}
