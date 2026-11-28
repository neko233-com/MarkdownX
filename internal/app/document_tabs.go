package app

import (
	"os"
	"path/filepath"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

const closedTabLimit = 20

type closedDocumentTab struct {
	panes         []closedDocumentPane
	index, active int
}

type closedDocumentPane struct {
	path   string
	draft  *documentTab
	weight float32
}

func newDocumentTab(d *doc.Document) *documentTab {
	core := editor.New(d)
	title := d.Title.Text
	source, ranges := d.WorkspaceMarkdown()
	state := &documentState{
		core: core, title: &title,
		savedGeneration: core.Generation, savedText: source,
		sourceDoc: d, sourceGen: core.Generation, sourceText: source, sourceRanges: ranges,
	}
	pane := newDocumentPane(state)
	state.selectionOwner = pane
	return pane
}

func newDocumentPane(state *documentState) *documentTab {
	pane := &documentTab{
		documentState: state, splitWeight: 1, selection: state.core.Sel,
		bufs: make(map[doc.BlockID]*blockBuffer),
		sel:  make(map[doc.BlockID][2]int), pending: make(map[doc.BlockID][2]int),
	}
	pane.captureSelectionText()
	return pane
}

func absoluteDocumentPath(path string) string {
	if path != "" {
		if absolute, err := filepath.Abs(path); err == nil {
			return absolute
		}
	}
	return path
}

func loadDocumentTab(path string) (*documentTab, error) {
	data, info, err := readEditorFile(path)
	if err != nil {
		return nil, err
	}
	var text string
	var file markdownFile
	if convert.Detect(path) == convert.FormatMarkdown {
		file = readMarkdownFile(path, data)
		file.identity = info
		text = file.text
	} else if text, err = convert.Markdown(path, data); err != nil {
		return nil, err
	}
	tab := newDocumentTab(doc.ReadSource(text))
	tab.path, tab.file = path, file
	return tab, nil
}

func (a *App) tabIndex() int {
	return slices.Index(a.tabs, a.documentTab.tabOwner())
}

func (a *App) tabAt(path string) *documentTab {
	if path != "" && len(a.tabs) != 0 {
		if a.path != "" && sameWorkspacePath(a.path, path) {
			return a.documentTab
		}
		for tab := range a.documentPanes {
			if tab.path != "" && sameWorkspacePath(tab.path, path) {
				return tab
			}
		}
		for tab := range a.documentPanes {
			if sameOpenFile(path, tab) {
				return tab
			}
		}
	}
	return nil
}

func (a *App) parkDocumentTab() {
	a.documentTab.claimSelection()
	a.syncAll()
	a.closeInlineSource(true)
	a.endWorkspaceTemplateSession()
	a.drag = nil
	a.core.BreakCoalescing()
	a.updateDirty()
	a.documentTab.rememberPaneSelection()
}

func (a *App) selectDocumentTab(tab *documentTab, focusEditor bool) {
	if tab == a.documentTab {
		return
	}
	a.parkDocumentTab()
	a.activateDocumentTab(tab, focusEditor)
}

func (a *App) activateDocumentTab(tab *documentTab, focusEditor bool) {
	a.documentTab = tab
	tab.ensureSourceView()
	tab.claimSelection()
	if tab.split != nil {
		tab.split.active = tab
		tab.split.reveal = tab
	}
	a.showOpenedDocument(false)
	if !focusEditor {
		return
	}
	a.focusPaneCaret()
}

func (a *App) insertDocumentTab(tab *documentTab, index int, focusEditor bool) {
	a.tabs = slices.Insert(a.tabs, index, tab)
	a.selectDocumentTab(tab, focusEditor)
}

func (a *App) openDocumentInNewTab(path string, index int) {
	path = absoluteDocumentPath(path)
	if tab := a.tabAt(path); tab != nil {
		a.selectDocumentTab(tab, false)
		return
	}
	tab, err := loadDocumentTab(path)
	if err != nil {
		a.status = "打开失败：" + err.Error()
		return
	}
	a.insertDocumentTab(tab, index, false)
}

func (a *App) newTab() {
	tab := newDocumentTab(doc.NewDocument())
	tab.revealTitle = a.mode != ModePreview
	a.insertDocumentTab(tab, a.tabIndex()+1, true)
}

func (a *App) stepTab(step int) {
	if len(a.tabs) > 1 {
		i := (a.tabIndex() + len(a.tabs) + step) % len(a.tabs)
		a.selectDocumentTab(a.tabs[i].activePane(), true)
	} else if step > 0 {
		a.nextWindow()
	}
}

func (a *App) rememberClosedTab(tab *documentTab, index int) {
	if tab.path != "" || tab.dirty {
		a.addClosedTab(closedDocumentTab{panes: []closedDocumentPane{a.closedPane(tab)}, index: index})
	}
}

func (a *App) closedPane(pane *documentTab) closedDocumentPane {
	rec := closedDocumentPane{path: pane.path, weight: pane.splitWeight}
	if rec.path == "" && pane.dirty {
		rec.draft = a.cloneDocumentPane(pane)
	}
	return rec
}

func (a *App) rememberClosedGroup(tab *documentTab, index int) {
	if tab.split == nil {
		a.rememberClosedTab(tab, index)
		return
	}
	record := closedDocumentTab{index: index, active: slices.Index(tab.split.panes, tab.split.active)}
	for _, pane := range tab.split.panes {
		record.panes = append(record.panes, a.closedPane(pane))
	}
	a.addClosedTab(record)
}

func (a *App) addClosedTab(record closedDocumentTab) {
	a.closedTabs = append(a.closedTabs, record)
	if len(a.closedTabs) > closedTabLimit {
		a.closedTabs = slices.Delete(a.closedTabs, 0, 1)
	}
}

func (a *App) reopenDocumentTab() bool {
	for len(a.closedTabs) > 0 {
		i := len(a.closedTabs) - 1
		record := a.closedTabs[i]
		a.closedTabs = slices.Delete(a.closedTabs, i, i+1)
		if len(record.panes) == 1 {
			if pane := a.tabAt(record.panes[0].path); pane != nil {
				a.selectDocumentTab(pane, true)
				return true
			}
		}
		owner, active := a.restoreClosedGroup(record)
		if owner == nil {
			continue
		}
		a.tabs = slices.Insert(a.tabs, min(record.index, len(a.tabs)), owner)
		a.selectDocumentTab(active, true)
		return true
	}
	return false
}

func (a *App) restoreClosedGroup(record closedDocumentTab) (owner, active *documentTab) {
	var panes []*documentTab
	restored := map[string]*documentTab{}
	for i, previous := range record.panes {
		var pane *documentTab
		switch {
		case previous.draft != nil:
			pane = previous.draft
		case previous.path == "":
			pane = newDocumentTab(doc.NewDocument())
		default:
			found := restored[workspacePathKey(previous.path)]
			if found == nil {
				found = a.tabAt(previous.path)
			}
			if found != nil {
				pane = a.cloneDocumentPane(found)
			} else {
				var err error
				pane, err = loadDocumentTab(previous.path)
				if err != nil {
					continue
				}
				restored[workspacePathKey(previous.path)] = pane
			}
		}
		pane.splitWeight = previous.weight
		panes = append(panes, pane)
		if i == record.active {
			active = pane
		}
	}
	if len(panes) == 0 {
		return nil, nil
	}
	owner = panes[0]
	if active == nil {
		active = owner
	}
	if len(panes) > 1 {
		split := &documentSplit{owner: owner, active: active, panes: panes}
		for _, pane := range panes {
			pane.split = split
		}
	}
	return owner, active
}

func (a *App) closeActiveTab() {
	a.closeDocumentTabs(a.documentTab.tabOwner())
}

func (a *App) confirmWindowClose() bool {
	return a.confirmClosingPanes(slices.Collect(a.documentPanes))
}

func (a *App) confirmClosingPanes(panes []*documentTab) bool {
	a.parkDocumentTab()
	kept := make(map[*documentState]bool)
	for _, window := range a.workspaceWindows() {
		for pane := range window.documentPanes {
			if !slices.Contains(panes, pane) {
				kept[pane.documentState] = true
			}
		}
	}
	for _, pane := range panes {
		if !kept[pane.documentState] && pane.dirty && !a.confirmUnsaved(pane) {
			return false
		}
		kept[pane.documentState] = true
	}
	return true
}

func (a *App) closeDocumentTabs(tabs ...*documentTab) {
	if len(tabs) == len(a.tabs) && a.win != nil {
		a.win.Close()
		return
	}
	previous, index := a.documentTab, a.tabIndex()
	var panes []*documentTab
	for _, tab := range tabs {
		panes = append(panes, tab.panes()...)
	}
	if !a.confirmClosingPanes(panes) {
		return
	}
	for i, tab := range a.tabs {
		if slices.Contains(tabs, tab) {
			a.rememberClosedGroup(tab, i)
		}
	}
	a.tabs = slices.DeleteFunc(a.tabs, func(tab *documentTab) bool { return slices.Contains(tabs, tab) })
	switch {
	case len(a.tabs) == 0:
		a.insertDocumentTab(newDocumentTab(doc.NewDocument()), 0, false)
	case slices.Contains(tabs, previous.tabOwner()):
		a.selectDocumentTab(a.tabs[min(index, len(a.tabs)-1)].activePane(), true)
	default:
		a.selectDocumentTab(previous, false)
	}
	a.services.Invalidate()
}

func (a *App) tabStrip(c *ui.Context) {
	t := c.Theme()
	active := a.documentTab.tabOwner()
	frame := c.TitleBar()
	bar := ui.Row(c.Key("document-tabs")).FillWidth().Height(max(36, frame.Height, frame.Bottom)).
		Padding(0, frame.Right+8, 0, frame.Left+8).Gap(4).Background(t.Surface).AlignItems(ui.Center).DragWindow()
	bar.Children(func() {
		var over bool
		scroll := ui.ScrollHorizontal(c.Key("document-tabs-scroll")).Grow(1).MinWidth(0).Height(30).
			TrackScroll(&a.tabScroll).Children(func() {
			ui.Row(c).Gap(4).Role(ui.RoleTabList).Label("打开的文档").Children(func() {
				for _, tab := range slices.Clone(a.tabs) {
					width := float32(168)
					if tab.split != nil {
						width = min(384, 128*float32(len(tab.split.panes)))
					}
					row := ui.Row(c.Key(tab)).Width(width).Height(28).Shrink(0).Radius(t.Radius).AlignItems(ui.Center)
					a.dragDocumentPart(row, tab, false)
					if tab == active {
						row.Background(t.Selection)
						if a.tabReveal {
							row.ScrollIntoView()
							a.tabReveal = false
						}
					}
					row.Children(func() {
						name, dirty := tab.tabLabel()
						label := name
						if dirty {
							label += " •"
						}
						tooltip := name
						if tab.split == nil {
							tooltip = firstNonEmpty(tab.path, name)
						}
						activate := ui.ButtonBase(c.Key("activate")).Role(ui.RoleTab).AccessSelected(tab == active).
							Label(label).Grow(1).MinWidth(0).Height(28).Padding(0, 8).Justify(ui.Start).
							Tooltip(tooltip)
						a.dragDocumentPart(activate, tab, false)
						activate.Children(func() { ui.Text(c, label).SingleLine().TruncateMiddle().FontSize(12) })
						if activate.Clicked() {
							a.selectDocumentTab(tab.activePane(), true)
						}
						if tabIconButton(c.Key("close"), "×", "关闭 "+name).Clicked() {
							a.closeDocumentTabs(tab)
						}
					})
					row.ContextMenu(func(menu *ui.Menu) { a.documentTabMenu(menu, tab) })
					over = a.dropOnTabStrip(c, row, tab) || over
				}
			})
		})
		if a.dropOnTabStrip(c, scroll, nil) || over {
			a.scrollTabsDuringDrag(c, scroll)
		}
		if tabIconButton(c.Key("new-tab"), "+", "新建标签页").Tooltip("新建标签页").Clicked() {
			a.newTab()
		}
	})
	a.dropOnTabStrip(c, bar, nil)
}

func tabIconButton(c *ui.Context, glyph, label string) ui.Element {
	t := c.Theme()
	button := ui.ButtonBase(c).Label(label).Size(26, 26).Radius(t.Radius).Center()
	if button.Hovered() {
		button.Background(t.Selection)
	}
	button.Children(func() {
		ui.Text(c, glyph).FontSize(16).TextColor(t.TextMuted).Role(ui.RoleNone)
	})
	return button
}

func (a *App) dropOnTabStrip(c *ui.Context, zone ui.Element, tab *documentTab) bool {
	dragged, tabOver := ui.DragOver[*documentDrag](zone)
	_, paneOver := ui.DragOver[splitPaneDrag](zone)
	tree, treeOver := ui.DragOver[*workspaceNode](zone)
	_, nativeOver := ui.DataDragOver(zone, workspaceFileDrop)
	filesOver := zone.FileDragOver()
	files := zone.DroppedFiles()
	index := len(a.tabs)
	before := false
	rtl := c.Direction() == ui.RightToLeft
	if tab != nil {
		index = slices.Index(a.tabs, tab)
		if index < 0 {
			return false
		}
		x, _, _ := zone.PointerPosition()
		// The legacy file event reports the target but has no pointer coordinates.
		before = !filesOver && len(files) == 0 && ((x < zone.Bounds().W/2) != rtl)
		if !before {
			index++
		}
	}
	over := treeOver || nativeOver || filesOver || paneOver
	if tabOver && dragged.tab != tab || over {
		hint := "在此打开标签页"
		if paneOver {
			hint = "分离为标签页"
		} else if tabOver {
			hint = "移动标签页到此处"
		} else if treeOver && tree.isDir() {
			hint = "打开工作区"
		}
		if tab != nil {
			color := c.Theme().Accent
			zone.DrawOver(func(p *ui.Painter, bounds ui.Rect) {
				x := bounds.X
				if before == rtl {
					x += bounds.W - 2
				}
				p.Fill(ui.Rect{X: x, Y: bounds.Y + 3, W: 2, H: bounds.H - 6}, color, 1)
			})
		}
		showWorkspaceDropHint(c, hint)
	}
	if dragged, dropped := ui.Drop[*documentDrag](zone); dropped {
		dragged.onDrop(func() { a.receiveDocumentDrag(dragged, false, index) })
		return false
	}
	if dragged, dropped := ui.Drop[splitPaneDrag](zone); dropped {
		dragged.onDrop(func() { a.receiveDocumentDrag(dragged.documentDrag, true, index) })
		return false
	}
	if tree, dropped := ui.Drop[*workspaceNode](zone); dropped {
		if err := a.checkDraggedTreeEntry(tree); err != nil {
			a.treeActionStatus(err)
			return false
		}
		a.openTabDrop([]string{tree.path}, index)
		return false
	}
	if drop, ok := ui.DropData(zone, workspaceFileDrop); ok {
		paths, err := drop.Data.Files()
		a.treeActionStatus(err)
		a.openTabDrop(paths, index)
		return false
	}
	a.openTabDrop(files, index)
	return over || tabOver
}

func (a *App) reorderDocumentTab(tab *documentTab, index int) {
	from := slices.Index(a.tabs, tab)
	if from < 0 || index == from || index == from+1 {
		return
	}
	a.tabs = slices.Delete(a.tabs, from, from+1)
	if from < index {
		index--
	}
	a.tabs = slices.Insert(a.tabs, index, tab)
	a.services.Invalidate()
}

func (a *App) openTabDrop(paths []string, index int) {
	for _, path := range paths {
		if !filepath.IsAbs(path) {
			continue
		}
		info, err := os.Stat(path)
		if err != nil {
			a.treeActionStatus(err)
			continue
		}
		if info.IsDir() {
			a.openWorkspaceFolder(path)
			continue
		}
		if !info.Mode().IsRegular() || convert.Detect(path) == convert.FormatUnknown {
			a.status = "无法在标签页中打开 " + filepath.Base(path)
			continue
		}
		count := len(a.tabs)
		a.openDocumentInNewTab(path, index)
		index += len(a.tabs) - count
	}
}

func (a *App) scrollTabsDuringDrag(c *ui.Context, viewport ui.Element) {
	x, _, inside := viewport.PointerPosition()
	width := viewport.Bounds().W
	if !inside || width < 48 {
		return
	}
	var to float32
	switch {
	case x < 24 && a.tabScroll.X > 0:
		to = max(0, a.tabScroll.X-9)
	case x > width-24 && a.tabScroll.X < a.tabScroll.MaxX:
		to = min(a.tabScroll.MaxX, a.tabScroll.X+9)
	default:
		return
	}
	a.tabScroll.X = to
	c.After(time.Second / 60)
}

func (a *App) documentTabMenu(menu *ui.Menu, tab *documentTab) {
	if menu.Item("在分栏中打开").Chosen() {
		a.insertSplitPane(a.cloneDocumentPane(tab.activePane()), -1)
	}
	if a.win != nil && a.windows != nil && menu.Item("将标签页移到新窗口").Chosen() {
		a.windows.detachDocument(&documentDrag{from: a, tab: tab}, false, nil)
	}
	if menu.Item("关闭标签页").Chosen() {
		a.closeDocumentTabs(tab)
	}
	if menu.Item("关闭其他标签页").Chosen() {
		others := slices.DeleteFunc(slices.Clone(a.tabs), func(other *documentTab) bool { return other == tab })
		a.closeDocumentTabs(others...)
	}
	i := slices.Index(a.tabs, tab)
	if i >= 0 && i+1 < len(a.tabs) && menu.Item("关闭右侧标签页").Chosen() {
		a.closeDocumentTabs(slices.Clone(a.tabs[i+1:])...)
	}
	if tab.path != "" {
		menu.Separator()
		a.workspacePathMenu(menu, tab.path)
	}
}

func (a *App) remapClosedTabs(moves workspacePathMoves) {
	for i := range a.closedTabs {
		record := &a.closedTabs[i]
		priorActive := record.active
		kept := record.panes[:0]
		for p, pane := range record.panes {
			from := pane.path
			pane.path = moves.path(from)
			if from != "" && pane.path == "" {
				if p < priorActive {
					record.active--
				}
				continue
			}
			kept = append(kept, pane)
		}
		clear(record.panes[len(kept):])
		record.panes = kept
		record.active = min(record.active, len(kept)-1)
	}
	a.closedTabs = slices.DeleteFunc(a.closedTabs, func(tab closedDocumentTab) bool {
		return len(tab.panes) == 0
	})
}

func sameOpenFile(path string, tab *documentTab) bool {
	if tab.file.identity == nil || path == "" || tab.path == "" || !samePath(path, tab.path) {
		return false
	}
	info, err := os.Stat(path)
	return err == nil && os.SameFile(info, tab.file.identity)
}
