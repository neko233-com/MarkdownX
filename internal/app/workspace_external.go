package app

import (
	"net/url"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func (a *App) workspaceExternalMenu(menu *ui.Menu, dir *workspaceNode) {
	menu.Submenu("在其他应用中打开", func(m *ui.Menu) {
		manager := "文件资源管理器"
		if runtime.GOOS == "darwin" {
			manager = "Finder"
		}
		if m.Item(manager).Disabled(a.win == nil).Chosen() {
			a.openWorkspaceIn(dir, mygo.Shell.OpenPath)
		}
		if m.Item("终端").Disabled(a.win == nil).Chosen() {
			a.openWorkspaceIn(dir, openWorkspaceTerminal)
		}
		if m.Item("Visual Studio Code").Disabled(a.win == nil).Chosen() {
			a.openWorkspaceIn(dir, func(path string) error {
				return mygo.Shell.OpenExternal(workspaceCodeURL(path))
			})
		}
	})
}

func (a *App) openWorkspaceIn(dir *workspaceNode, open func(string) error) {
	if err := a.checkTreeNode(dir); err != nil {
		a.treeActionStatus(err)
		return
	}
	a.treeActionStatus(open(dir.path))
}

func workspaceCodeURL(dir string) string {
	path := filepath.ToSlash(dir)
	if !strings.HasPrefix(path, "/") {
		path = "/" + path
	}
	u := url.URL{Scheme: "vscode", Host: "file", Path: strings.TrimRight(path, "/") + "/"}
	return u.String()
}
