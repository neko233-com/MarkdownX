package app

import (
	"cmp"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/convert"
)

type workspaceNode struct {
	path       string
	info       fs.FileInfo
	children   []*workspaceNode
	childPaths []string
	tracked    bool
}

func (n *workspaceNode) name() string { return n.info.Name() }
func (n *workspaceNode) isDir() bool  { return n.info.IsDir() }

func (n *workspaceNode) at(path string) *workspaceNode {
	if n == nil {
		return nil
	}
	rel, inside := workspaceRel(n.path, path)
	if !inside || rel == "." {
		if inside {
			return n
		}
		return nil
	}
	names := strings.Split(rel, string(filepath.Separator))
	for _, name := range names {
		at := slices.IndexFunc(n.children, func(child *workspaceNode) bool {
			return workspacePathKey(name) == workspacePathKey(child.name())
		})
		if at == -1 {
			return nil
		}
		n = n.children[at]
	}
	return n
}

func (n *workspaceNode) index(nodes map[string]*workspaceNode) int {
	nodes[n.path] = n
	depth := 0
	for _, child := range n.children {
		depth = max(depth, 1+child.index(nodes))
	}
	return depth
}

func (a *App) refreshFiles() {
	a.treeRevision++
	previousFile := a.activeWorkspacePath()
	previousNodes := a.treeNodes
	previousRoot := ""
	if a.tree != nil {
		previousRoot = a.tree.path
	}
	a.tree, a.files = readWorkspace(a.root, a.treeFilter)
	a.treeOrder.sort(a.tree)
	if a.tree == nil {
		a.treeNodes, a.treeView = nil, nil
		a.treeReveal = ""
		a.treeSearch.stop()
		return
	}
	a.treeNodes = make(map[string]*workspaceNode, len(a.files)+1)
	depth := a.tree.index(a.treeNodes)
	changedRoot := workspacePathKey(previousRoot) != workspacePathKey(a.tree.path)
	if changedRoot {
		a.treeReveal, a.treeTarget = "", ""
		a.treeUndo = workspaceUndo{}
		a.treeView = &workspaceTreeView{cursor: -1}
		a.treeView.Open.Add(a.tree.path)
	} else {
		a.treeView.rememberCursor()
		a.reconcileWorkspacePaths(previousNodes)
	}
	a.treeView.maxDepth = depth
	a.treeView.Generation++
	if changedRoot || previousFile == "" {
		a.revealWorkspaceFile()
	}
}

func workspacePathKey(path string) string {
	if runtime.GOOS == "windows" {
		return strings.ToLower(path)
	}
	return path
}

func workspaceFolderPath(path string) (string, bool) {
	if path == "" {
		return "", false
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", false
	}
	info, err := os.Lstat(abs)
	if err != nil {
		return "", false
	}
	if info.IsDir() {
		return abs, true
	}
	if info.Mode()&os.ModeSymlink == 0 {
		return "", false
	}
	// WalkDir does not traverse a link used as its root. Resolve only that
	// link so regular paths keep their spelling and nested links stay leaves.
	resolved, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return "", false
	}
	info, err = os.Stat(resolved)
	return resolved, err == nil && info.IsDir()
}

func readWorkspace(root string, filter workspaceFilter) (*workspaceNode, []string) {
	if root == "" {
		return nil, nil
	}
	root = filepath.Clean(root)
	dirs := make(map[string]*workspaceNode)
	var files []string
	rules := filter.rules()
	_ = filepath.WalkDir(root, func(p string, item os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return nil
		}
		name := item.Name()
		rel, _ := filepath.Rel(root, p)
		if p != root {
			if item.IsDir() && rules.pruneDirectory(name, rel) {
				return filepath.SkipDir
			}
			if !item.IsDir() && (!item.Type().IsRegular() || !rules.showFile(name, rel)) {
				return nil
			}
		}
		info, err := item.Info()
		if err != nil {
			return nil
		}
		if p != root && !filter.ShowHidden && workspaceHidden(info) {
			if info.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		node := &workspaceNode{path: p, info: info}
		if item.IsDir() {
			dirs[p] = node
		} else {
			files = append(files, rel)
		}
		if parent := dirs[filepath.Dir(p)]; parent != nil && parent != node {
			parent.children = append(parent.children, node)
		}
		return nil
	})
	slices.SortFunc(files, naturalPathOrder)
	return dirs[root], files
}

// naturalPathOrder compares numbers as runs, without converting arbitrarily
// long numbers in a file name to an integer.
func naturalPathOrder(a, b string) int {
	left, right := strings.ToLower(a), strings.ToLower(b)
	for left != "" && right != "" {
		if asciiDigit(left[0]) && asciiDigit(right[0]) {
			i, j := 0, 0
			for i < len(left) && asciiDigit(left[i]) {
				i++
			}
			for j < len(right) && asciiDigit(right[j]) {
				j++
			}
			l, r := strings.TrimLeft(left[:i], "0"), strings.TrimLeft(right[:j], "0")
			if order := cmp.Compare(len(l), len(r)); order != 0 {
				return order
			}
			if order := strings.Compare(l, r); order != 0 {
				return order
			}
			left, right = left[i:], right[j:]
			continue
		}
		if order := cmp.Compare(left[0], right[0]); order != 0 {
			return order
		}
		left, right = left[1:], right[1:]
	}
	if order := cmp.Compare(len(left), len(right)); order != 0 {
		return order
	}
	return strings.Compare(a, b)
}

func asciiDigit(b byte) bool { return b >= '0' && b <= '9' }

func (a *App) activeWorkspacePath() string {
	node := a.treeNodes[a.path]
	if node == nil && a.path != "" && runtime.GOOS == "windows" {
		node = a.tree.at(a.path)
	}
	if node != nil && !node.isDir() {
		return node.path
	}
	return ""
}

func (a *App) revealWorkspaceFile() {
	path := a.activeWorkspacePath()
	if path != "" {
		a.revealTreePath(path)
	}
}

func (a *App) revealTreePath(path string) {
	if a.expandWorkspaceParents(path) {
		a.treeReveal = path
	}
}

func (a *App) expandWorkspaceParents(path string) bool {
	if a.tree == nil || path == "" {
		return false
	}
	if _, inside := workspaceRel(a.tree.path, path); !inside {
		return false
	}
	if !sameWorkspacePath(path, a.tree.path) {
		for dir := filepath.Dir(path); ; dir = filepath.Dir(dir) {
			a.treeView.Open.Add(dir)
			a.treeNodes[dir].trackIdentity()
			if sameWorkspacePath(dir, a.tree.path) {
				break
			}
		}
	}
	return true
}

func (a *App) showWorkspaceTree() {
	a.focusWorkspaceTree()
	a.remember()
}

func (a *App) focusWorkspaceTree() {
	a.setSidebarPage(sidebarFiles)
	a.treeSearch.open = false
	a.treeSearch.stop()
	a.treeFocus = true
	a.treeTarget = ""
	a.revealWorkspaceFile()
}

func (a *App) workspacePane(c *ui.Context, width float32) {
	if a.tree == nil {
		pane := ui.Column(c.Key("no-workspace")).Grow(1).Fill().Children(func() {
			e := ui.Button(c.Key("choose-root"), "打开文件夹…")
			if a.treeFocus {
				e.Focus()
				a.treeFocus = false
			}
			if e.Clicked() {
				a.ChooseFolder()
			}
			ui.MenuButton(c.Key("workspace-history"), "最近和固定的文件夹", a.workspaceFolderMenus).FontSize(12)
		})
		a.dropEmptyWorkspace(c, pane)
		return
	}
	if a.treeSearch.open {
		a.workspaceSearchPane(c)
		return
	}
	a.treeDropSeen = false
	root := a.tree
	view := a.treeView
	first, last := view.List.Visible()
	pinned := first > 0
	if pinned && !view.menuPinned && first <= view.cursor && view.cursor <= last {
		view.List.ScrollIntoView(view.cursor)
	}
	view.menuPinned = pinned
	if pinned {
		header := a.workspaceRootMenu(c.Key("pinned-workspace-menu"), root).FillWidth().Margin(0, 0, 2, 0)
		if a.tree != root {
			a.services.Invalidate()
			return
		}
		a.dropOnTreeDirectory(c, header, root, false)
		if a.tree != root {
			return
		}
	}
	scroll := a.workspaceOutline(c, root, width)
	if a.tree != root {
		return
	}
	a.dropOnTreeDirectory(c, scroll, root, false)
	if !a.treeDropSeen {
		a.treeDropTarget, a.treeDropCopy = "", false
	}
}

func (a *App) openTreeFile(path string) {
	a.openWorkspaceFile(path, workspaceOpenCurrent)
}

func (a *App) openWorkspaceFile(path string, mode workspaceOpenMode) {
	if convert.Detect(path) == convert.FormatUnknown {
		a.status = "可从右键菜单用系统应用打开 " + filepath.Base(path)
		return
	}
	switch mode {
	case workspaceOpenTab:
		a.openDocumentInNewTab(path, a.tabIndex()+1)
	case workspaceOpenWindow:
		if a.windows != nil {
			if a.windows.showOpenFile(path) == nil {
				a.windows.open(path, false)
			}
		} else {
			a.openDocumentInNewTab(path, a.tabIndex()+1)
		}
	default:
		if a.path == "" || !sameWorkspacePath(a.path, path) {
			a.OpenFile(path)
		}
	}
}

func (a *App) treeMenu(menu *ui.Menu, node *workspaceNode) {
	if !node.isDir() {
		if convert.Detect(node.path) != convert.FormatUnknown {
			if menu.Item("打开").Chosen() {
				a.openTreeFile(node.path)
			}
			menu.Submenu("打开方式", func(sub *ui.Menu) {
				if sub.Item("在当前标签页中打开").Chosen() {
					a.openWorkspaceFile(node.path, workspaceOpenCurrent)
				}
				if sub.Item("在新标签页中打开").Chosen() {
					a.openWorkspaceFile(node.path, workspaceOpenTab)
				}
				if sub.Item("在分栏中打开").Chosen() {
					if err := a.checkTreeNode(node); err != nil {
						a.treeActionStatus(err)
					} else {
						a.openDocumentInSplit(node.path, -1)
					}
				}
				if a.windows != nil && sub.Item("在新窗口中打开").Chosen() {
					a.windows.open(node.path, false)
				}
			})
		} else if a.win != nil && menu.Item("用系统应用打开").Chosen() {
			if err := a.checkTreeNode(node); err != nil {
				a.treeActionStatus(err)
			} else {
				a.treeActionStatus(mygo.Shell.OpenPath(node.path))
			}
		}
		menu.Separator()
	}
	a.treeFileMenu(menu, node)
	menu.Separator()
	a.workspacePathMenu(menu, node.path)
	if node.isDir() {
		a.workspaceExternalMenu(menu, node)
		menu.Separator()
		if menu.Item("切换文件夹…").Chosen() {
			a.ChooseFolder()
		}
		if menu.Item("刷新文件树").Chosen() {
			a.refreshFiles()
		}
		a.workspacePinMenu(menu, node.path)
		if sameWorkspacePath(node.path, a.root) {
			if menu.Item("搜索工作区…").Shortcut(ui.Cmd|ui.Shift, ui.KeyF).Chosen() {
				a.showWorkspaceSearch()
			}
			if menu.Item("筛选…").Chosen() {
				a.treeFilterEdit = workspaceFilterDialog{open: true, value: a.treeFilter}
			}
			menu.Submenu("排序", a.treeSortMenu)
			a.workspaceFolderMenus(menu)
			if menu.Item("关闭文件夹").Chosen() {
				a.closeWorkspaceFolder()
			}
		}
	}
}

func (a *App) workspacePathMenu(menu *ui.Menu, path string) {
	if menu.Item("复制路径").Chosen() {
		a.services.WriteClipboard(path)
	}
	if rel, ok := a.workspaceRelativePath(path); ok && menu.Item("复制相对路径").Chosen() {
		a.services.WriteClipboard(filepath.ToSlash(rel))
	}
	if menu.Item("在文件管理器中显示").Chosen() {
		mygo.Shell.ShowItemInFolder(path)
	}
}

func (a *App) checkWorkspace(c *ui.Context) {
	if a.treeWatch != nil && a.treeWatch.root != a.root {
		a.stopWorkspaceWatch()
	}
	if a.root == "" {
		return
	}
	if a.win != nil && a.tree != nil && a.treeWatch == nil {
		a.treeWatch = watchWorkspace(a.root, a.treeFilter, c.Services())
	}
	if watch := a.treeWatch; watch != nil {
		if watch.changed.Swap(false) {
			a.refreshFiles()
		}
		if !watch.stopped.Load() {
			return
		}
	}
	now := c.Now()
	if !now.Before(a.treeCheckAt) {
		a.treeCheckAt = now.Add(time.Second)
		if a.tree == nil || a.visibleDirectoryChanged(a.tree) {
			missing := a.tree == nil
			a.refreshFiles()
			if missing && a.tree != nil {
				a.stopWorkspaceWatch()
				a.services.Invalidate()
			}
		}
	}
	c.After(a.treeCheckAt.Sub(now))
}

func (a *App) visibleDirectoryChanged(node *workspaceNode) bool {
	info, err := os.Stat(node.path)
	if err != nil || !info.IsDir() || !info.ModTime().Equal(node.info.ModTime()) {
		return true
	}
	if a.treeView.Open.Has(node.path) {
		for _, child := range node.children {
			if child.isDir() && a.visibleDirectoryChanged(child) {
				return true
			}
			if !child.isDir() && a.treeOrder.By == workspaceModified {
				info, err := os.Stat(child.path)
				if err != nil || !info.ModTime().Equal(child.info.ModTime()) {
					return true
				}
			}
		}
	}
	return false
}
