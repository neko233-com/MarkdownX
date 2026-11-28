package app

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func workspaceDropPaths(paths []string) []string {
	seen := make(map[string]bool, len(paths))
	unique := make([]string, 0, len(paths))
	for _, path := range paths {
		if path == "" {
			continue
		}
		path = filepath.Clean(path)
		key := workspacePathKey(path)
		if !seen[key] {
			seen[key] = true
			unique = append(unique, path)
		}
	}
	result := unique[:0]
	for _, path := range unique {
		// A selected folder already includes all its descendants.
		nested := false
		for child, parent := path, filepath.Dir(path); child != parent; child, parent = parent, filepath.Dir(parent) {
			if seen[workspacePathKey(parent)] {
				nested = true
				break
			}
		}
		if !nested {
			result = append(result, path)
		}
	}
	return result
}

func (a *App) routeWorkspaceDrop(dir *workspaceNode, paths []string, copyDrop bool) {
	if len(paths) == 0 {
		return
	}
	if err := a.checkTreeNode(dir); err != nil {
		a.treeActionStatus(err)
		return
	}
	var undo workspaceUndo
	var copies []string
	var lastPath string
	var failed int
	var failure error
	rules := a.treeFilter.rules()
	for _, path := range workspaceDropPaths(paths) {
		_, inside := workspaceRel(a.root, path)
		moving := inside && !copyDrop
		if moving && sameWorkspacePath(filepath.Dir(path), dir.path) {
			continue
		}
		entry, err := a.workspaceDropEntry(dir, path, moving, rules)
		if err == nil {
			to := filepath.Join(dir.path, entry.name())
			if moving {
				var moved workspaceMoveUndo
				moved, err = a.renameWorkspaceEntry(entry, to)
				if err == nil {
					undo.moves = append(undo.moves, moved)
				}
			} else {
				if sameWorkspacePath(entry.path, to) {
					to, err = duplicateWorkspacePath(entry)
				} else {
					err = copyWorkspacePath(entry, to)
				}
				if err == nil {
					copies = append(copies, to)
				}
			}
			if err == nil {
				lastPath = to
			}
		}
		if err != nil {
			failed++
			if failure == nil {
				failure = fmt.Errorf("%s: %w", filepath.Base(path), err)
			}
		}
	}
	if lastPath == "" {
		a.treeActionStatus(failure)
		return
	}
	var warning error
	if len(undo.moves) > 0 {
		warning = a.applyWorkspaceMoves(undo.moves)
	} else {
		a.refreshFiles()
	}
	undo.copies = workspaceCopiedEntries(copies)
	a.treeUndo = undo
	a.focusTreeEntry(lastPath)
	var status []string
	if len(undo.moves) > 0 {
		if len(undo.moves) == 1 && len(copies) == 0 && failure == nil {
			status = append(status, "已移动 "+filepath.Base(lastPath))
		} else {
			status = append(status, "已移动 "+itoa(len(undo.moves))+" 个条目")
		}
	}
	if len(copies) > 0 {
		status = append(status, "已复制 "+itoa(len(copies))+" 个条目")
	}
	if failure != nil {
		status = append(status, itoa(failed)+" 个条目未完成："+failure.Error())
	}
	a.reportWorkspaceImageChanges(strings.Join(status, " · "), warning)
}

func (a *App) workspaceDropEntry(dir *workspaceNode, path string, moving bool, rules workspaceRules) (*workspaceNode, error) {
	if moving {
		if node := a.tree.at(path); node != nil {
			if !canMoveTreeEntry(node, dir) {
				return nil, errors.New("请选择另一个目标文件夹")
			}
			return node, a.checkTreeNode(node)
		}
	}
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	parent, _ := workspaceRel(a.root, dir.path)
	name := info.Name()
	rel := filepath.Join(parent, name)
	_, containsTarget := workspaceRel(path, dir.path)
	if !rules.ShowHidden && workspaceHidden(info) ||
		!(info.IsDir() && !containsTarget && !rules.pruneDirectory(name, rel) || info.Mode().IsRegular() && rules.showFile(name, rel)) {
		return nil, errUnsupportedWorkspaceImport
	}
	node := &workspaceNode{path: path, info: info}
	if moving {
		return node, a.checkTreeNode(node)
	}
	return node, nil
}
