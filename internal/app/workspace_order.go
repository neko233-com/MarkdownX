package app

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
)

type workspaceSort uint8

const (
	workspaceNatural workspaceSort = iota
	workspaceAlphabetical
	workspaceModified
	workspaceCreated
)

type workspaceOrder struct {
	By         workspaceSort `json:"by,omitempty"`
	Descending bool          `json:"descending,omitempty"`
	Mix        bool          `json:"mix_folders_and_files,omitempty"`
}

func (order workspaceOrder) sort(node *workspaceNode) {
	if node == nil || !node.isDir() {
		return
	}
	slices.SortFunc(node.children, func(a, b *workspaceNode) int {
		if !order.Mix && a.isDir() != b.isDir() {
			if a.isDir() {
				return -1
			}
			return 1
		}
		var result int
		switch order.By {
		case workspaceAlphabetical:
			result = strings.Compare(strings.ToLower(a.name()), strings.ToLower(b.name()))
		case workspaceModified:
			result = a.info.ModTime().Compare(b.info.ModTime())
		case workspaceCreated:
			result = workspaceCreationTime(a.info).Compare(workspaceCreationTime(b.info))
		}
		if result == 0 {
			result = naturalPathOrder(a.name(), b.name())
		}
		if order.Descending {
			result = -result
		}
		return result
	})
	if node.childPaths == nil || len(node.childPaths) != len(node.children) {
		node.childPaths = make([]string, len(node.children))
	}
	for i, child := range node.children {
		node.childPaths[i] = child.path
		order.sort(child)
	}
}

func (a *App) treeSortMenu(menu *ui.Menu) {
	order := a.treeOrder
	if menu.Item("文件夹排在前面").Checked(!order.Mix).Chosen() {
		order.Mix = !order.Mix
	}
	menu.Separator()
	for i, label := range []string{"自然顺序", "名称", "修改时间", "创建时间"} {
		if menu.Item(label).Radio(order.By == workspaceSort(i)).Chosen() {
			order.By = workspaceSort(i)
		}
	}
	menu.Separator()
	if menu.Item("升序").Radio(!order.Descending).Chosen() {
		order.Descending = false
	}
	if menu.Item("降序").Radio(order.Descending).Chosen() {
		order.Descending = true
	}
	if order != a.treeOrder {
		a.treeView.rememberCursor()
		a.treeOrder = order
		order.sort(a.tree)
		a.treeView.Generation++
		s := loadSettings()
		s.TreeOrder = order
		saveSettings(s)
		a.services.Invalidate()
	}
}
