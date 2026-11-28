package app

import "github.com/egoist/mygo/ui"

const (
	defaultSidebarWidth   float32 = 264
	minSidebarWidth       float32 = 156
	minEditorWidth        float32 = 320
	sidebarGripWidth      float32 = 6
	sidebarResizeStep     float32 = 10
	sidebarPadding        float32 = 6
	workspaceFooterHeight float32 = 30
)

type sidebarPage uint8

const (
	sidebarFiles sidebarPage = iota
	sidebarOutline
)

func (a *App) setSidebarPage(page sidebarPage) {
	a.sidebarPage, a.hideSide, a.zenMode = page, false, false
	if page == sidebarOutline {
		a.treeSearch.open = false
		a.treeSearch.stop()
		a.treeFocus, a.treeTarget = false, ""
		a.treeDropTarget, a.treeDropCopy = "", false
		if a.treeView != nil {
			a.treeView.closeContextMenu()
		}
	} else {
		a.outline.focus = false
	}
}

func (a *App) changeSidebarPage(c *ui.Context, page sidebarPage) {
	a.setSidebarPage(page)
	a.remember()
	c.Invalidate()
}

func (a *App) sidebarTabs(c *ui.Context) {
	selected := int(a.sidebarPage)
	t := c.Theme()
	labels := [...]string{"文件", "大纲"}
	tabs := ui.TabsBase(c.Key("sidebar-pages"), &selected, len(labels))
	tabs.List.Label("侧栏视图").FillWidth().Gap(4).Margin(0, 0, 6, 0).Children(func() {
		for i, label := range labels {
			tab := tabs.Tab(i).Label(label).Height(32).MinWidth(0).Grow(1).Center().Radius(t.Radius)
			color := t.TextMuted
			if selected == i {
				tab.Background(t.Selection)
				color = t.Text
			} else if tab.Hovered() {
				color = t.Text
			}
			tab.Children(func() { ui.Text(c, label).SingleLine().FontSize(13).TextColor(color).Role(ui.RoleNone) })
		}
	})
	if selected != int(a.sidebarPage) {
		a.changeSidebarPage(c, sidebarPage(selected))
	}
}

func (a *App) sidebarFooter(c *ui.Context) {
	t := c.Theme()
	nextPage, name, label := sidebarOutline, "大纲", "切换到大纲"
	if a.sidebarPage == sidebarOutline {
		nextPage, name, label = sidebarFiles, "文件", "切换到文件树"
	}
	ui.Row(c.Key("sidebar-footer")).Height(workspaceFooterHeight).Shrink(0).Gap(4).AlignItems(ui.Center).Children(func() {
		switchPage := ui.ButtonBase(c.Key("sidebar-page-switch")).Height(24).Padding(0, 8).Radius(t.Radius).
			Label(label).Tooltip(label)
		if switchPage.Hovered() {
			switchPage.Background(t.Selection)
		}
		switchPage.Children(func() { ui.Text(c, name).FontSize(12).TextColor(t.Accent).Role(ui.RoleNone) })
		if switchPage.Clicked() {
			a.changeSidebarPage(c, nextPage)
		}
		if root := a.tree; a.sidebarPage == sidebarFiles && root != nil && !a.treeSearch.open {
			ui.ButtonBase(c.Key("workspace-add")).Label("新建文件或文件夹").Tooltip("新建文件或文件夹").
				Size(28, 24).Center().PopoverMenuButton(func(menu *ui.Menu) { a.treeCreateMenu(menu, root, root) }).
				Children(func() { ui.Text(c, "+").FontSize(17).Role(ui.RoleNone) })
		}
	})
}

func (a *App) sidebar(c *ui.Context) {
	a.checkWorkspace(c)
	windowWidth, _ := c.Size()
	width, shown, minWidth, maxWidth := a.sidebarWidths(windowWidth)
	t := c.Theme()
	ui.Column(c.Key("workspace-sidebar")).Width(width).Shrink(0).FillHeight().Clip().
		Background(t.Surface).Padding(8, sidebarPadding, 0, sidebarPadding).Children(func() {
		a.sidebarTabs(c)
		ui.Column(c.Key("sidebar-content")).Grow(1).Fill().MinHeight(0).Margin(0, 0, 4, 0).Children(func() {
			if a.sidebarPage == sidebarOutline {
				a.documentOutlinePane(c)
			} else {
				a.workspacePane(c, width-2*sidebarPadding)
			}
		})
		a.sidebarFooter(c)
	})
	grip := ui.Box(c.Key("workspace-resize")).Width(sidebarGripWidth).Shrink(0).FillHeight().
		Focusable().Role(ui.RoleSplitter).Label("调整侧栏宽度").Orientation(ui.Vertical).
		Range(float64(minWidth), float64(maxWidth), float64(width)).Step(float64(sidebarResizeStep)).
		Cursor(ui.CursorResizeEW).Tooltip("拖动调整侧栏宽度，双击恢复默认")
	delta, _, dragging := grip.Dragged()
	left, right := ui.KeyLeft, ui.KeyRight
	if c.Direction() == ui.RightToLeft {
		delta, left, right = -delta, right, left
	}
	switch {
	case grip.DoubleClicked():
		a.saveSidebarWidth(defaultSidebarWidth)
	case width > minWidth && grip.Shortcut(0, left):
		a.saveSidebarWidth(max(minWidth, width-sidebarResizeStep))
	case width < maxWidth && grip.Shortcut(0, right):
		a.saveSidebarWidth(min(maxWidth, width+sidebarResizeStep))
	case dragging && delta != 0:
		if a.sidebarDrag == nil {
			a.sidebarDrag = new(width)
		}
		*a.sidebarDrag += delta
		c.Invalidate()
	case !dragging && a.sidebarDrag != nil:
		if _, _, ended := grip.DragEnded(); ended && width != shown {
			a.saveSidebarWidth(width)
		}
		a.sidebarDrag = nil
		c.Invalidate()
	}
	color := t.Border
	if dragging || grip.Hovered() {
		color = t.Accent
	}
	grip.Draw(func(p *ui.Painter, r ui.Rect) {
		p.Fill(ui.Rect{X: r.X + (r.W-1)/2, Y: r.Y, W: 1, H: r.H}, color, 0)
	})
}

func (a *App) sidebarWidths(windowWidth float32) (width, shown, minWidth, maxWidth float32) {
	maxWidth = max(0, windowWidth-minEditorWidth-sidebarGripWidth)
	minWidth = min(minSidebarWidth, maxWidth)
	shown = min(max(a.sidebarWidth, minWidth), maxWidth)
	width = shown
	if a.sidebarDrag != nil {
		width = min(max(*a.sidebarDrag, minWidth), maxWidth)
	}
	return
}

func (a *App) saveSidebarWidth(width float32) {
	if width <= 0 || width == a.sidebarWidth {
		return
	}
	a.sidebarWidth = width
	s := loadSettings()
	s.SidebarWidth = width
	saveSettings(s)
	a.services.Invalidate()
}
