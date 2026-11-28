package app

import (
	"path/filepath"

	"github.com/egoist/mygo/ui"
)

const (
	workspaceRowHeight     float32 = 26
	workspaceArrowWidth    float32 = 18
	workspaceRowPadding    float32 = 8
	workspaceMinLabelWidth float32 = 72
	workspaceIconWidth     float32 = 18
	workspaceStartIndent   float32 = 4
	workspaceLevelIndent   float32 = 16
)

type workspaceTreeView struct {
	ui.OutlineState[string]
	cursor      int
	keepPath    string
	keepVisible bool
	menuPinned  bool
	menuPath    string
	menuOpen    bool
	maxDepth    int
}

func (view *workspaceTreeView) rememberCursor() {
	if view.keepPath == "" {
		view.keepPath = view.selectedPath()
		first, last := view.List.Visible()
		view.keepVisible = first <= view.cursor && view.cursor <= last
	}
}

func (view *workspaceTreeView) rowIndex(path string) int {
	for i := range view.Rows() {
		if view.Item(i) == path {
			return i
		}
	}
	return -1
}

func (view *workspaceTreeView) selectedPath() string {
	if view == nil || view.cursor < 0 || view.cursor >= view.Rows() {
		return ""
	}
	return view.Item(view.cursor)
}

func (view *workspaceTreeView) revealForContextMenu(ev ui.InputEvent) bool {
	menuKey := ev.Key == ui.KeyContextMenu && ev.Mods == 0 || ev.Key == ui.KeyF10 && ev.Mods == ui.Shift
	if ev.Kind != ui.InputKeyDown || !menuKey {
		return false
	}
	path := view.selectedPath()
	first, last := view.List.Visible()
	if path == "" || first <= view.cursor && view.cursor <= last {
		return false
	}
	view.menuPath, view.menuOpen = path, true
	view.List.ScrollIntoView(view.cursor)
	return true
}

func (view *workspaceTreeView) closeContextMenu() {
	view.menuPath, view.menuOpen = "", false
}

func (a *App) workspaceOutline(c *ui.Context, root *workspaceNode, width float32) ui.Element {
	view, nodes := a.treeView, a.treeNodes
	if !view.menuOpen || view.menuPath != view.selectedPath() || nodes[view.menuPath] == nil {
		view.closeContextMenu()
	}
	view.List.Selected = &view.cursor
	view.List.Label = func(i int) string {
		return nodes[view.Item(i)].name()
	}
	// Outline calls Prefix immediately before the content of the same row.
	var disclosure ui.OutlineRow
	view.Prefix = func(r ui.OutlineRow) { disclosure = r }
	maxIndent := max(0, width-workspaceArrowWidth-workspaceIconWidth-workspaceRowPadding-workspaceMinLabelWidth)
	// Size the indentation once for the scanned tree so scrolling does not shift its columns.
	levelIndent := min(workspaceLevelIndent, max(2, (maxIndent-workspaceStartIndent)/float32(max(1, view.maxDepth))))
	active := a.activeWorkspacePath()
	var doubleClick bool
	viewport := ui.Outline(c.Key("workspace"), &view.OutlineState, []string{root.path}, func(path string) []string {
		return nodes[path].childPaths
	}, func(path string) {
		node := nodes[path]
		buildMenu := func(menu *ui.Menu) { a.treeMenu(menu, node) }
		if node.isDir() && view.Open.Has(path) {
			node.trackIdentity()
		}
		row := ui.Row(c).Grow(1).FillWidth().Shrink(1).MinWidth(0).Height(workspaceRowHeight).AlignItems(ui.Center).PaddingEnd(workspaceRowPadding).
			Radius(c.Theme().Radius).Description(path)
		if path == active && path != view.selectedPath() {
			row.Background(c.Theme().Selection).TextColor(c.Theme().Text)
		}
		row.Children(func() {
			indent := min(workspaceStartIndent+float32(disclosure.Depth)*levelIndent, maxIndent)
			workspaceDisclosure(c, disclosure, indent)
			workspaceEntryIcon(c, node.isDir())
			if node == root {
				a.workspaceRootMenu(c.Key("workspace-menu"), node).Grow(1)
			} else {
				ui.Text(c, node.name()).FontSize(13).SingleLine().Grow(1).MinWidth(0).TruncateMiddle()
			}
			if path == view.menuPath {
				// An offscreen row has no native menu anchor until it is laid out again.
				ui.Box(c.Key("workspace-context-anchor")).Absolute().Top(0).Start(0).Size(0, workspaceRowHeight).
					Role(ui.RoleNone).TabStop(false).PopoverMenuButton(buildMenu, ui.PopoverMenuButtonOptions{Open: &view.menuOpen})
			}
		})
		if (row.Clicked() || row.RowClicked()) && a.tree == root {
			a.treeFocus, a.treeTarget = true, path
			a.activateWorkspaceNode(node)
		}
		if row.RowDoubleClicked() {
			doubleClick = true
		}
		row.ContextMenu(buildMenu)
		if a.tree != root {
			return
		}
		if node != root {
			a.dragTreeEntry(row, node)
		}
		if node.isDir() {
			if node != root {
				row.Tooltip(node.path)
			}
			a.dropOnTreeDirectory(c, row, node, true)
		} else {
			a.dropOnTreeDirectory(c, row, nodes[filepath.Dir(path)], false)
			a.previewWorkspaceFile(c, row, node)
		}
	}).Grow(1).Fill().Label("文件目录树").FocusRing(false).
		HandleInput(view.revealForContextMenu).
		ContextMenu(func(menu *ui.Menu) { a.treeMenu(menu, root) })
	if a.tree != root {
		a.services.Invalidate()
		return viewport
	}
	if node := nodes[view.selectedPath()]; node != nil {
		if viewport.Submitted() && !doubleClick || viewport.Shortcut(0, ui.KeySpace) {
			a.activateWorkspaceNode(node)
		}
		a.treeActionKeys(viewport, node)
	}
	a.revealWorkspaceRow(view, viewport, root, active)
	nodes[view.selectedPath()].trackIdentity()
	return viewport
}

func workspaceEntryIcon(c *ui.Context, folder bool) {
	color := c.Theme().TextMuted
	ui.Box(c).Size(workspaceIconWidth, 16).Shrink(0).Role(ui.RoleNone).Draw(func(p *ui.Painter, r ui.Rect) {
		x, y := r.X+1, r.Y+2
		var mark ui.Path
		if folder {
			mark.MoveTo(x+1, y+4).LineTo(x+1, y+2).LineTo(x+5, y+2).LineTo(x+7, y+4).
				LineTo(x+13, y+4).LineTo(x+13, y+12).LineTo(x+1, y+12).LineTo(x+1, y+4)
		} else {
			mark.MoveTo(x+3, y+1).LineTo(x+9, y+1).LineTo(x+12, y+4).LineTo(x+12, y+13).
				LineTo(x+3, y+13).LineTo(x+3, y+1)
			mark.MoveTo(x+6, y+7).LineTo(x+9, y+7).MoveTo(x+6, y+10).LineTo(x+9, y+10)
		}
		p.StrokePath(&mark, 1.2, color)
	})
}

func (a *App) workspaceRootMenu(c *ui.Context, root *workspaceNode) ui.Element {
	return ui.MenuButton(c, root.name(), func(menu *ui.Menu) { a.treeMenu(menu, root) }).
		Label("文件夹操作").Description(root.path).Tooltip(root.path).
		Height(24).MinWidth(0).Padding(0, 8).Justify(ui.SpaceBetween).FontSize(13)
}

func workspaceDisclosure(c *ui.Context, r ui.OutlineRow, indent float32) {
	ui.Box(c).Width(indent).Shrink(0)
	arrow := r.Arrow().Size(workspaceArrowWidth, workspaceRowHeight)
	if !r.Branch {
		return
	}
	color := c.Theme().TextMuted
	direction := float32(1)
	if c.Direction() == ui.RightToLeft {
		direction = -1
	}
	arrow.Draw(func(p *ui.Painter, rect ui.Rect) {
		x, y := rect.X+rect.W/2, rect.Y+rect.H/2
		var path ui.Path
		if r.Open {
			path.MoveTo(x-3, y-2).LineTo(x, y+2).LineTo(x+3, y-2)
		} else {
			path.MoveTo(x-2*direction, y-3).LineTo(x+2*direction, y).LineTo(x-2*direction, y+3)
		}
		p.StrokePath(&path, 1.5, color)
	})
}

func (a *App) activateWorkspaceNode(node *workspaceNode) {
	if !node.isDir() {
		a.openTreeFile(node.path)
		return
	}
	open := &a.treeView.Open
	if open.Has(node.path) {
		open.Remove(node.path)
	} else {
		open.Add(node.path)
	}
	a.services.Invalidate()
}

func (a *App) revealWorkspaceRow(view *workspaceTreeView, viewport ui.Element, root *workspaceNode, active string) {
	path := a.treeReveal
	if a.treeFocus {
		path = firstNonEmpty(a.treeTarget, active, root.path)
	}
	if view.keepPath != "" {
		i := view.rowIndex(view.keepPath)
		view.List.Choose(i)
		if i >= 0 && view.keepVisible && path == "" {
			view.List.ScrollIntoView(i)
		}
		view.keepPath = ""
		a.services.Invalidate()
	}
	if path == "" {
		return
	}
	i := view.rowIndex(path)
	if i < 0 && a.treeFocus && view.Rows() > 0 {
		i = 0
	}
	if i >= 0 {
		view.List.ScrollIntoView(i)
		if a.treeFocus {
			view.List.Choose(i)
			viewport.Focus()
		}
	}
	a.treeFocus, a.treeTarget, a.treeReveal = false, "", ""
	a.services.Invalidate()
}
