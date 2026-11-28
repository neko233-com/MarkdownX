package app

import (
	"path/filepath"
	"slices"

	"github.com/egoist/mygo/ui"
)

func recentWorkspaceFolders(history []string, path string) []string {
	out := []string{path}
	for _, folder := range history {
		if len(out) == recentLimit {
			break
		}
		if !sameWorkspacePath(folder, path) {
			out = append(out, folder)
		}
	}
	return out
}

func workspaceFolderListed(folders []string, path string) bool {
	return slices.ContainsFunc(folders, func(folder string) bool { return sameWorkspacePath(folder, path) })
}

func withoutWorkspaceFolder(folders []string, path string) []string {
	return slices.DeleteFunc(folders, func(folder string) bool { return sameWorkspacePath(folder, path) })
}

func (a *App) setWorkspacePinned(path string, pinned bool) {
	s := loadSettings()
	s.PinnedFolders = withoutWorkspaceFolder(s.PinnedFolders, path)
	if pinned {
		s.PinnedFolders = append([]string{path}, s.PinnedFolders...)
		a.status = "已固定 " + filepath.Base(path)
	} else {
		a.status = "已取消固定 " + filepath.Base(path)
	}
	saveSettings(s)
	a.services.Invalidate()
}

func (a *App) workspacePinMenu(menu *ui.Menu, path string) {
	pinned := workspaceFolderListed(loadSettings().PinnedFolders, path)
	label := "固定文件夹"
	if pinned {
		label = "取消固定文件夹"
	}
	if menu.Item(label).Chosen() {
		a.setWorkspacePinned(path, !pinned)
	}
}

func (a *App) workspaceFolderMenus(menu *ui.Menu) {
	s := loadSettings()
	menu.Submenu("固定的文件夹", func(sub *ui.Menu) {
		a.workspaceFolderList(sub, s.PinnedFolders, true)
	})
	recent := slices.DeleteFunc(s.RecentFolders, func(path string) bool {
		return workspaceFolderListed(s.PinnedFolders, path)
	})
	menu.Submenu("最近的文件夹", func(sub *ui.Menu) {
		a.workspaceFolderList(sub, recent, false)
	})
}

func (a *App) workspaceFolderList(menu *ui.Menu, folders []string, pinned bool) {
	if len(folders) == 0 {
		menu.Item("暂无记录").Disabled(true)
		return
	}
	for _, path := range folders {
		menu.Submenu(path, func(sub *ui.Menu) {
			if sub.Item("打开文件夹").Chosen() {
				a.openWorkspaceFolder(path)
			}
			if pinned {
				if sub.Item("取消固定").Chosen() {
					a.setWorkspacePinned(path, false)
				}
			} else {
				if sub.Item("固定到文件夹菜单").Chosen() {
					a.setWorkspacePinned(path, true)
				}
				if sub.Item("从最近记录移除").Chosen() {
					s := loadSettings()
					s.RecentFolders = withoutWorkspaceFolder(s.RecentFolders, path)
					saveSettings(s)
				}
			}
		})
	}
}
