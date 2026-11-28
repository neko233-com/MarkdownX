package app

import (
	"os"
	"path/filepath"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/convert"
)

const (
	splitEdgeWidth    float32 = 12
	splitDividerWidth float32 = 10
	splitMinPaneWidth float32 = 216
	splitResizeStep   float32 = 12
)

type splitPaneDrag struct {
	*documentDrag
}

func (a *App) documentPanesView(c *ui.Context) {
	pane := a.documentTab
	split := pane.split
	if split == nil {
		if len(a.tabs) == 1 {
			a.documentPaneView(c, pane)
			return
		}
		var drop func()
		ui.Row(c.Key(pane)).MinWidth(0).Grow(1).Fill().Children(func() {
			left := ui.Box(c.Key("split-edge-left")).Width(splitEdgeWidth).Shrink(0).FillHeight().Label("左侧分栏落点")
			drop, _ = a.splitDropTarget(c, left, nil, 0)
			ui.Column(c.Key("single-pane")).MinWidth(0).Grow(1).Fill().Children(func() { a.documentPaneView(c, pane) })
			right := ui.Box(c.Key("split-edge-right")).Width(splitEdgeWidth).Shrink(0).FillHeight().Label("右侧分栏落点")
			if action, _ := a.splitDropTarget(c, right, nil, 1); action != nil {
				drop = action
			}
		})
		if drop != nil {
			drop()
		}
		return
	}

	windowWidth, _ := c.Size()
	available := windowWidth
	if !a.hideSide {
		sidebarWidth, _, _, _ := a.sidebarWidths(windowWidth)
		available -= sidebarWidth + sidebarGripWidth
	}
	widths, total := splitPaneWidths(split.panes, available)
	var drop func()
	var dragging bool
	addDrop := func(zone ui.Element, index int) {
		action, over := a.splitDropTarget(c, zone, split, index)
		dragging = dragging || over
		if action != nil {
			drop = action
		}
	}
	viewport := ui.ScrollHorizontal(c.Key(split)).Grow(1).MinWidth(0).Fill().
		TrackScroll(&split.scroll).Label("文档分栏")
	viewport.Children(func() {
		ui.Row(c).Width(total).Shrink(0).FillHeight().Children(func() {
			addDrop(ui.Box(c.Key("split-edge-left")).Width(splitEdgeWidth).Shrink(0).FillHeight().Label("左侧分栏落点"), 0)
			for i, child := range slices.Clone(split.panes) {
				if i > 0 {
					addDrop(a.splitDivider(c, split, child, i-1, widths[i-1], widths[i]), i)
				}
				column := ui.Column(c.Key(child)).Width(widths[i]).Shrink(0).FillHeight().Clip().
					Role(ui.RoleRegion).Label("分栏 " + child.DisplayName())
				if split.reveal == child {
					column.ScrollIntoView()
					split.reveal = nil
				}
				column.Children(func() {
					action, over := a.splitPaneHeader(c, child, i)
					if action != nil {
						drop = action
					}
					dragging = dragging || over
					a.documentPaneView(c, child)
				})
			}
			addDrop(ui.Box(c.Key("split-edge-right")).Width(splitEdgeWidth).Shrink(0).FillHeight().Label("右侧分栏落点"), len(split.panes))
		})
	})
	if dragging {
		scrollSplitDrag(c, viewport, &split.scroll)
	}
	if drop != nil {
		drop()
	}
}

func splitPaneWidths(panes []*documentTab, available float32) ([]float32, float32) {
	n := float32(len(panes))
	chrome := splitEdgeWidth*2 + splitDividerWidth*(n-1)
	base := max(available-chrome, n*splitMinPaneWidth)
	if len(panes) > 2 {
		base = max(base, n*(available-splitEdgeWidth*2-splitDividerWidth)/2)
	}
	var sum float32
	for _, pane := range panes {
		sum += pane.splitWeight
	}
	widths := make([]float32, len(panes))
	for i, pane := range panes {
		weight := 1 / n
		if sum != 0 {
			weight = pane.splitWeight / sum
		}
		widths[i] = splitMinPaneWidth + (base-n*splitMinPaneWidth)*weight
	}
	return widths, base + chrome
}

func (a *App) splitDivider(c *ui.Context, split *documentSplit, right *documentTab, index int, leftWidth, rightWidth float32) ui.Element {
	left := split.panes[index]
	divider := ui.Box(c.Key(struct{ right *documentTab }{right})).
		Width(splitDividerWidth).Shrink(0).FillHeight().
		Label("调整 "+left.DisplayName()+" 与 "+right.DisplayName()+" 的分栏宽度").
		Role(ui.RoleSplitter).Orientation(ui.Vertical).Focusable().Cursor(ui.CursorResizeEW).
		Range(float64(splitMinPaneWidth), float64(leftWidth+rightWidth-splitMinPaneWidth), float64(leftWidth)).
		Step(float64(splitResizeStep))
	delta, _, dragging := divider.Dragged()
	if !dragging {
		if divider.Shortcut(0, ui.KeyLeft) {
			delta = -splitResizeStep
		}
		if divider.Shortcut(0, ui.KeyRight) {
			delta = splitResizeStep
		}
	}
	if c.Direction() == ui.RightToLeft {
		delta = -delta
	}
	if divider.DoubleClicked() {
		delta = (rightWidth - leftWidth) / 2
	}
	if delta != 0 {
		extra := leftWidth + rightWidth - splitMinPaneWidth*2
		if extra > 0 {
			pool := left.splitWeight + right.splitWeight
			left.splitWeight = pool * min(max(leftWidth+delta-splitMinPaneWidth, 0), extra) / extra
			right.splitWeight = pool - left.splitWeight
			c.Invalidate()
		}
	}
	color := c.Theme().Border
	if dragging || divider.Hovered() {
		color = c.Theme().Accent
	}
	divider.Draw(func(p *ui.Painter, r ui.Rect) {
		p.Fill(ui.Rect{X: r.X + (r.W-1)/2, Y: r.Y, W: 1, H: r.H}, color, 0)
	})
	return divider
}

func (a *App) splitPaneHeader(c *ui.Context, pane *documentTab, index int) (func(), bool) {
	t := c.Theme()
	title := pane.DisplayName()
	if pane.dirty {
		title += " •"
	}
	bar := ui.Row(c.Key("split-pane-header")).Height(30).Padding(0, 4).FillWidth().AlignItems(ui.Center).
		Background(t.Surface)
	a.dragDocumentPart(bar, pane, true)
	if pane == pane.split.active {
		bar.Background(t.Selection)
	}
	var action func()
	bar.Children(func() {
		button := ui.ButtonBase(c.Key("split-pane-select")).Grow(1).MinWidth(0).Height(28).Padding(0, 6).
			Justify(ui.Start).Label("聚焦分栏 " + title).Tooltip(firstNonEmpty(pane.path, title))
		a.dragDocumentPart(button, pane, true)
		button.Children(func() { ui.Text(c, title).SingleLine().TruncateMiddle().FontSize(12) })
		if button.Clicked() {
			action = func() { a.selectDocumentTab(pane, true) }
		}
		if tabIconButton(c.Key("close-pane"), "×", "关闭分栏 "+pane.DisplayName()).Clicked() {
			action = func() { a.closeSplitPane(pane) }
		}
	})
	_, over := ui.DragOver[splitPaneDrag](bar)
	if over {
		showWorkspaceDropHint(c, "移动分栏到此处")
	}
	if dragged, ok := ui.Drop[splitPaneDrag](bar); ok {
		x, _, _ := bar.PointerPosition()
		if (x >= bar.Bounds().W/2) != (c.Direction() == ui.RightToLeft) {
			index++
		}
		split := pane.split
		return func() { dragged.onDrop(func() { a.receiveSplitPaneDrag(dragged.documentDrag, split, index) }) }, true
	}
	return action, over
}

func (a *App) splitDropTarget(c *ui.Context, zone ui.Element, split *documentSplit, index int) (func(), bool) {
	_, paneOver := ui.DragOver[splitPaneDrag](zone)
	entry, treeOver := ui.DragOver[*workspaceNode](zone)
	_, nativeOver := ui.DataDragOver(zone, workspaceFileDrop)
	fileOver := zone.FileDragOver()
	over := paneOver || nativeOver || fileOver || treeOver && !entry.isDir() && convert.Detect(entry.path) != convert.FormatUnknown
	if over {
		zone.Background(c.Theme().Selection)
		hint := "在此打开分栏"
		if paneOver {
			hint = "移动分栏到此处"
		}
		showWorkspaceDropHint(c, hint)
	}
	if drag, ok := ui.Drop[splitPaneDrag](zone); ok {
		return func() { drag.onDrop(func() { a.receiveSplitPaneDrag(drag.documentDrag, split, index) }) }, over
	}
	if entry, ok := ui.Drop[*workspaceNode](zone); ok {
		return func() {
			if err := a.checkDraggedTreeEntry(entry); err != nil {
				a.treeActionStatus(err)
				return
			}
			a.openSplitDrop([]string{entry.path}, index)
		}, over
	}
	if native, ok := ui.DropData(zone, workspaceFileDrop); ok {
		paths, err := native.Data.Files()
		return func() {
			a.treeActionStatus(err)
			a.openSplitDrop(paths, index)
		}, over
	}
	if files := zone.DroppedFiles(); len(files) > 0 {
		return func() { a.openSplitDrop(files, index) }, over
	}
	return nil, over
}

func (a *App) openSplitDrop(paths []string, index int) {
	for _, path := range workspaceDropPaths(paths) {
		if !filepath.IsAbs(path) || convert.Detect(path) == convert.FormatUnknown {
			a.status = "无法在分栏中打开 " + filepath.Base(path)
			continue
		}
		info, err := os.Stat(path)
		if err != nil || !info.Mode().IsRegular() {
			a.status = "无法在分栏中打开 " + filepath.Base(path)
			continue
		}
		count := len(a.documentTab.panes())
		a.openDocumentInSplit(path, index)
		index += len(a.documentTab.panes()) - count
	}
}

func (a *App) reorderSplitPane(split *documentSplit, pane *documentTab, index int) {
	if split == nil || pane.split != split {
		return
	}
	from := slices.Index(split.panes, pane)
	if from < 0 || index == from || index == from+1 {
		return
	}
	split.panes = slices.Delete(split.panes, from, from+1)
	if from < index {
		index--
	}
	split.panes = slices.Insert(split.panes, index, pane)
	a.services.Invalidate()
}

func scrollSplitDrag(c *ui.Context, viewport ui.Element, scroll *ui.ScrollState) {
	x, _, over := viewport.PointerPosition()
	width := viewport.Bounds().W
	if !over || width < 60 {
		return
	}
	if c.Direction() == ui.RightToLeft {
		x = width - x
	}
	switch {
	case x < 30 && scroll.X > 0:
		scroll.X = max(0, scroll.X-10)
	case x > width-30 && scroll.X < scroll.MaxX:
		scroll.X = min(scroll.MaxX, scroll.X+10)
	default:
		return
	}
	c.After(time.Second / 60)
}

func (a *App) activatePaneInput(pane *documentTab) bool {
	if pane == a.documentTab {
		pane.claimSelection()
		return true
	}
	if pane.split == nil || pane.split != a.split {
		return false
	}
	a.selectDocumentTab(pane, false)
	a.services.Invalidate()
	return true
}

func (a *App) observePaneInput(pane *documentTab, ev ui.InputEvent) {
	switch ev.Kind {
	case ui.InputPointerDown, ui.InputKeyDown, ui.InputText:
		a.activatePaneInput(pane)
	}
}
