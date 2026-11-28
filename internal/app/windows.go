package app

import (
	"os"
	"slices"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type desktopWindows struct {
	order  []*App
	active *App
	closed []string
	ready  bool
}

// Run starts the desktop editor.
func Run(path string) error {
	app := mygo.App
	app.SetName("MarkdownX")
	desktop := &desktopWindows{}
	app.OnOpenFile(desktop.openFile)
	app.OnActivate(func(visible bool) {
		if desktop.ready && !visible && len(desktop.order) == 0 {
			desktop.open("", false)
		}
	})
	app.WhenReady(func() {
		app.SetMenu(desktop.menu())
		desktop.open(path, path == "")
		desktop.ready = true
	})
	return app.Run()
}

func (d *desktopWindows) open(path string, restore bool) *App {
	a := New(path)
	a.windows = d
	if restore {
		a.restoreWorkspace()
	} else if path == "" && d.active != nil {
		a.root = d.active.root
		a.refreshFiles()
		a.resetPreview()
	}
	d.showWindow(a, nil)
	return a
}

const (
	desktopWindowWidth     = 1120
	desktopWindowHeight    = 760
	desktopWindowMinWidth  = 760
	desktopWindowMinHeight = 420
)

func (d *desktopWindows) showWindow(a *App, point *mygo.Point) {
	options := mygo.WindowOptions{
		Title: "MarkdownX", Width: desktopWindowWidth, Height: desktopWindowHeight,
		MinWidth: desktopWindowMinWidth, MinHeight: desktopWindowMinHeight,
		TitleBarStyle: mygo.TitleBarDefault,
		Content:       ui.View(a.View),
	}
	if point != nil {
		display := mygo.Screen.DisplayNearestPoint(*point)
		bounds := detachedWindowBounds(*point, display.WorkArea)
		options.Position = &mygo.Point{X: bounds.X, Y: bounds.Y}
		options.Width, options.Height = bounds.Width, bounds.Height
	}
	w := mygo.NewWindow(options)
	a.win = w
	d.order = append(d.order, a)
	d.active = a
	w.OnFocus(func() { d.active = a })
	w.OnClose(func(e *mygo.CloseEvent) {
		if len(a.tabs) != 0 && !a.confirmWindowClose() {
			e.PreventDefault()
		}
	})
	w.OnClosed(func() { d.remove(a) })
	a.SyncTitle()
}

func (d *desktopWindows) remove(a *App) {
	a.stopWorkspaceWatch()
	a.treeSearch.stop()
	for tab := range a.openDocuments {
		if tab != a.documentTab {
			d.rememberClosed(tab.path)
		}
	}
	if len(a.tabs) != 0 {
		d.rememberClosed(a.path)
	}
	d.order = slices.DeleteFunc(d.order, func(s *App) bool { return s == a })
	if d.active == a {
		d.active = nil
		if len(d.order) > 0 {
			d.active = d.order[len(d.order)-1]
		}
	}
}

func (d *desktopWindows) rememberClosed(path string) {
	if path != "" {
		d.closed = pushRecent(d.closed, path)
	}
}

func (d *desktopWindows) openFile(path string) {
	d.activateFile(path)
}

func (d *desktopWindows) activateFile(path string) *App {
	path = absoluteDocumentPath(path)
	if a := d.active; a != nil {
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			a.openWorkspaceFolder(path)
			a.services.Invalidate()
			return a
		}
	}
	if a := d.showOpenFile(path); a != nil {
		return a
	}
	if a := d.active; a != nil && len(a.tabs) != 0 && a.path == "" && !a.dirty {
		a.OpenFile(path)
		a.services.Invalidate()
		return a
	}
	return d.open(path, false)
}

func (d *desktopWindows) showOpenFile(path string) *App {
	if a := d.active; a != nil {
		if tab := a.tabAt(path); tab != nil {
			return d.focusTab(a, tab)
		}
	}
	for _, a := range d.order {
		if a != d.active {
			if tab := a.tabAt(path); tab != nil {
				return d.focusTab(a, tab)
			}
		}
	}
	return nil
}

func (d *desktopWindows) focusTab(a *App, tab *documentTab) *App {
	a.selectDocumentTab(tab, true)
	if a.win != nil {
		a.win.Focus()
	}
	a.services.Invalidate()
	return a
}

func (d *desktopWindows) state(w *mygo.Window) *App {
	for _, a := range d.order {
		if a.win == w && len(a.tabs) != 0 {
			return a
		}
	}
	if d.active != nil && len(d.active.tabs) != 0 {
		return d.active
	}
	return nil
}

func (d *desktopWindows) nextClosed() string {
	for len(d.closed) > 0 {
		path := d.closed[0]
		d.closed = d.closed[1:]
		if info, err := os.Stat(path); err == nil && !info.IsDir() {
			return path
		}
	}
	return ""
}

func (a *App) newWindow() {
	if a.windows != nil {
		a.windows.open("", false)
	}
}

func (a *App) reopenClosed() {
	if a.reopenDocumentTab() {
		return
	}
	if a.windows == nil {
		return
	}
	if path := a.windows.nextClosed(); path != "" {
		a.windows.openFile(path)
	} else {
		a.status = "没有可重新打开的文件"
	}
}

func (a *App) nextWindow() {
	if a.windows == nil || len(a.windows.order) < 2 {
		return
	}
	order := a.windows.order
	at := slices.Index(order, a)
	if w := order[(at+1)%len(order)].win; w != nil {
		w.Focus()
	}
}

func (d *desktopWindows) menu() *mygo.Menu {
	items := []*mygo.MenuItem{
		{Label: "MarkdownX", Submenu: []*mygo.MenuItem{{Role: mygo.RoleAbout}, mygo.Separator(), {Role: mygo.RoleQuit}}},
	}
	for _, label := range []string{"文件", "编辑", "段落", "格式", "视图"} {
		var entries []*mygo.MenuItem
		if label == "编辑" {
			entries = []*mygo.MenuItem{
				{Role: mygo.RoleCut}, {Role: mygo.RoleCopy}, {Role: mygo.RolePaste}, {Role: mygo.RoleSelectAll},
				mygo.Separator(),
			}
		}
		for _, cmd := range commands {
			if cmd.menu != label {
				continue
			}
			entries = append(entries, &mygo.MenuItem{
				Label: cmd.label, Accelerator: cmd.accelerator,
				Click: func(_ *mygo.MenuItem, w *mygo.Window) {
					a := d.state(w)
					if a == nil {
						if cmd.label == "新建" || cmd.label == "新建窗口" || cmd.label == "新建标签页" {
							d.open("", false)
						}
						return
					}
					if a.accepts(cmd) {
						a.documentTab.claimSelection()
						cmd.run(a)
						a.services.Invalidate()
					}
				},
			})
		}
		items = append(items, &mygo.MenuItem{Label: label, Submenu: entries})
	}
	items = append(items, &mygo.MenuItem{Role: mygo.RoleWindowMenu})
	return mygo.NewMenu(items)
}
