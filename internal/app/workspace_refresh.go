package app

import (
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/convert"
)

func (n *workspaceNode) trackIdentity() {
	if n != nil && !n.tracked {
		// On Windows SameFile loads the ID while the old path still exists.
		n.tracked = os.SameFile(n.info, n.info)
	}
}

func (a *App) reconcileWorkspacePaths(previous map[string]*workspaceNode) {
	view := a.treeView
	openFiles := make(map[string]fs.FileInfo, len(a.tabs))
	for tab := range a.openDocuments {
		if _, inside := workspaceRel(a.root, tab.path); inside && tab.file.identity != nil {
			openFiles[tab.path] = tab.file.identity
		}
	}
	var added []*workspaceNode
	collected := false
	findMoved := func(path string, info fs.FileInfo) *workspaceNode {
		if !collected {
			collected = true
			for path, n := range a.treeNodes {
				if previous[path] == nil {
					added = append(added, n)
				}
			}
		}
		return findMovedWorkspaceEntry(path, info, added)
	}
	resolved := make(map[string]*workspaceNode)
	resolve := func(path string) *workspaceNode {
		if path == "" {
			return nil
		}
		if n, checked := resolved[path]; checked {
			return n
		}
		n := a.treeNodes[path]
		if n == nil && runtime.GOOS == "windows" {
			n = a.tree.at(path)
		}
		if n == nil {
			old := previous[path]
			switch {
			case openFiles[path] != nil:
				// A recent save can have replaced the file since the tree scan.
				n = findMoved(path, openFiles[path])
			case old != nil && old.tracked:
				n = findMoved(path, old.info)
			}
		}
		resolved[path] = n
		return n
	}

	var open ui.Selection[string]
	for path := range view.Open.All() {
		if n := resolve(path); n != nil && n.isDir() {
			n.trackIdentity()
			path = n.path
		}
		open.Add(path)
	}
	view.Open = open
	if old := view.keepPath; old != "" {
		for path := old; ; path = filepath.Dir(path) {
			if n := resolve(path); n != nil {
				view.keepPath = n.path
				n.trackIdentity()
				break
			}
			if filepath.Dir(path) == path {
				break
			}
		}
		if view.keepPath != old {
			a.expandWorkspaceParents(view.keepPath)
		}
	}
	if n := resolve(a.treeTarget); n != nil {
		a.treeTarget = n.path
	}
	if n := resolve(a.treeReveal); n != nil {
		a.treeReveal = n.path
	}
	for tab := range a.openDocuments {
		path := tab.path
		if identity := openFiles[path]; identity != nil && a.treeNodes[path] == nil {
			if n := resolve(path); n != nil && n.path != path && os.SameFile(identity, n.info) {
				a.followMovedWorkspaceDocument(tab, n)
			}
		}
	}
}

func findMovedWorkspaceEntry(priorPath string, prior fs.FileInfo, added []*workspaceNode) *workspaceNode {
	if len(added) == 0 {
		return nil
	}
	var spellingOnly bool
	switch info, err := os.Lstat(priorPath); {
	case err == nil && os.SameFile(prior, info):
		// A case-insensitive volume can still open a file via its old spelling.
		spellingOnly = true
	case !os.IsNotExist(err):
		return nil
	}
	var match *workspaceNode
	for _, n := range added {
		if prior.IsDir() != n.isDir() || spellingOnly && !strings.EqualFold(priorPath, n.path) || !os.SameFile(prior, n.info) {
			continue
		}
		if match != nil {
			return nil // Two destinations might be hard links to the same file.
		}
		match = n
	}
	return match
}

func (a *App) followMovedWorkspaceDocument(tab *documentTab, n *workspaceNode) {
	if convert.Detect(n.path) != convert.FormatMarkdown || a.checkTreeNode(n) != nil {
		return
	}
	move := workspacePathMove{from: tab.path, to: n.path}
	tab.path, tab.file.path = n.path, n.path
	tab.status = "已跟随移到 " + n.name()
	a.remapClosedTabs(workspacePathMoves{move})
	s := loadSettings()
	s.Last = move.path(s.Last)
	s.Recent = remapRecentPaths(s.Recent, move.path)
	a.recents = s.Recent
	saveSettings(s)
	if tab != a.documentTab {
		return
	}
	a.resetPreview()
	if a.treeView.keepPath == "" || a.treeView.keepPath == n.path {
		a.revealWorkspaceFile()
	} else {
		a.expandWorkspaceParents(n.path)
	}
}
