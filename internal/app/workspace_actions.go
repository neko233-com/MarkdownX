package app

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"unicode"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
)

type workspaceUndo struct {
	moves  []workspaceMoveUndo
	copies []workspaceCopy
}

type workspaceMoveUndo struct {
	from, to  string
	info      fs.FileInfo
	parent    fs.FileInfo
	fromTrash bool
	images    map[string]workspaceImageChange
}

func (u workspaceUndo) available() bool { return len(u.moves) > 0 || len(u.copies) > 0 }

var errUnsupportedWorkspaceImport = errors.New("条目类型或当前筛选条件不支持导入")
var errWorkspaceEntryChanged = errors.New("条目已在外部更改，请从文件树重新选择")

func workspaceRel(root, path string) (string, bool) {
	if root == "" || path == "" {
		return "", false
	}
	rel, err := filepath.Rel(root, path)
	return rel, err == nil && filepath.IsLocal(rel)
}

func sameWorkspacePath(a, b string) bool {
	return workspacePathKey(filepath.Clean(a)) == workspacePathKey(filepath.Clean(b))
}

func (a *App) checkTreeNode(node *workspaceNode) error {
	rel, inside := workspaceRel(a.root, node.path)
	if !inside {
		return errors.New("条目已不在当前工作区")
	}
	physicalRoot, err := filepath.EvalSymlinks(a.root)
	if err != nil {
		return err
	}
	physicalPath, err := filepath.EvalSymlinks(node.path)
	if err != nil {
		return err
	}
	if !sameWorkspacePath(filepath.Join(physicalRoot, rel), physicalPath) {
		return errors.New("条目已经移至链接指向的位置")
	}
	info, err := os.Lstat(node.path)
	if err != nil {
		return err
	}
	if os.SameFile(node.info, info) {
		return nil
	}
	// A save can replace the active file before the tree receives its watch event.
	if a.file.identity != nil && sameWorkspacePath(node.path, a.file.path) && os.SameFile(a.file.identity, info) {
		node.info, node.tracked = info, true
		return nil
	}
	return errWorkspaceEntryChanged
}

func workspaceEntryName(name string) (string, error) {
	name = strings.TrimSpace(name)
	if !filepath.IsLocal(name) || name == "." || name == ".." ||
		strings.ContainsAny(name, `\/:`) || strings.ContainsFunc(name, unicode.IsControl) ||
		runtime.GOOS == "windows" && (strings.HasSuffix(name, ".") || strings.ContainsAny(name, `<>"|?*`)) {
		return "", errors.New("请输入有效的文件或文件夹名称")
	}
	return name, nil
}

func (a *App) focusTreeEntry(path string) {
	if a.tree != nil {
		for a.tree.at(path) == nil {
			parent := filepath.Dir(path)
			if path == parent || path == "" {
				path = a.tree.path
				break
			}
			path = parent
		}
		if node := a.tree.at(path); node != nil {
			path = node.path
		}
	}
	a.treeFocus, a.treeTarget = true, path
	a.revealTreePath(path)
	a.services.Invalidate()
}

func workspaceMarkdownName(name string) string {
	if convert.Detect(name) != convert.FormatMarkdown {
		return name + loadSettings().TreeDocumentExt
	}
	return name
}

func (a *App) createTreeEntry(dir *workspaceNode, name string, folder bool, body string) (string, error) {
	if err := a.checkTreeNode(dir); err != nil {
		return "", err
	}
	name, err := workspaceEntryName(name)
	if err != nil {
		return "", err
	}
	if !folder {
		name = workspaceMarkdownName(name)
	}
	path := filepath.Join(dir.path, name)
	if folder {
		err = os.Mkdir(path, 0o777)
	} else {
		err = writeNewWorkspaceDocument(path, body)
	}
	if errors.Is(err, fs.ErrExist) {
		return "", fmt.Errorf("目标目录已有 %q", name)
	}
	if err != nil {
		return "", err
	}
	a.treeUndo = workspaceUndo{}
	a.refreshFiles()
	a.focusTreeEntry(path)
	if !folder {
		a.openTreeFile(path)
	}
	a.status = "已新建 " + name
	return path, nil
}

func writeNewWorkspaceDocument(path, body string) error {
	if body == "" {
		file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o666)
		if err != nil {
			return err
		}
		return file.Close()
	}
	// Stage the template in the destination directory so a failed write or
	// a concurrent create cannot publish a partial file or replace a note.
	file, err := os.CreateTemp(filepath.Dir(path), ".markdownx-*.tmp")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	defer file.Close()
	if _, err := file.WriteString(body); err != nil {
		return err
	}
	if err := file.Sync(); err != nil {
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	return moveWorkspacePath(file.Name(), path)
}

func (a *App) renameTreeEntry(node *workspaceNode, name string) error {
	if err := a.checkTreeNode(node); err != nil {
		return err
	}
	name, err := workspaceEntryName(name)
	if err != nil {
		return err
	}
	if !node.isDir() {
		if filepath.Ext(name) == "" {
			name += filepath.Ext(node.name())
		}
		if convert.Detect(name) != convert.Detect(node.name()) {
			return errors.New("重命名时需要保留原有文件格式")
		}
	}
	to := filepath.Join(filepath.Dir(node.path), name)
	if to == node.path {
		a.focusTreeEntry(to)
		return nil
	}
	return a.moveTreeEntry(node, to, "已重命名为 "+name)
}

func canMoveTreeEntry(node, dir *workspaceNode) bool {
	_, descendant := workspaceRel(node.path, dir.path)
	return dir.isDir() && !descendant && !sameWorkspacePath(filepath.Dir(node.path), dir.path)
}

func (a *App) moveTreeEntry(node *workspaceNode, to, status string) error {
	entry, err := a.renameWorkspaceEntry(node, to)
	if err != nil {
		return err
	}
	moves := []workspaceMoveUndo{entry}
	warning := a.applyWorkspaceMoves(moves)
	a.treeUndo = workspaceUndo{moves: moves}
	a.focusTreeEntry(to)
	a.reportWorkspaceImageChanges(status, warning)
	return nil
}

func (a *App) renameWorkspaceEntry(node *workspaceNode, to string) (workspaceMoveUndo, error) {
	if sameWorkspacePath(node.path, a.root) {
		return workspaceMoveUndo{}, errors.New("无法移动当前工作区的根目录")
	}
	parent, err := os.Stat(filepath.Dir(node.path))
	if err != nil {
		return workspaceMoveUndo{}, err
	}
	// Stat can defer loading the Windows file ID. Keep the identity before
	// a later external replacement of the directory.
	if !os.SameFile(parent, parent) {
		return workspaceMoveUndo{}, errors.New("无法读取源目录标识")
	}
	if err := moveWorkspacePath(node.path, to); err != nil {
		return workspaceMoveUndo{}, err
	}
	return workspaceMoveUndo{from: to, to: node.path, info: node.info, parent: parent}, nil
}

func (a *App) applyWorkspaceMoves(entries []workspaceMoveUndo) error {
	moves := make(workspacePathMoves, len(entries))
	for i, entry := range entries {
		moves[i] = workspacePathMove{from: entry.to, to: entry.from}
	}
	changes, warning := a.rebaseWorkspaceImages(moves, nil)
	for i := range entries {
		entry := &entries[i]
		if info := changes[entry.from].identity; info != nil {
			entry.info = info
		}
		for path, change := range changes {
			if _, inside := workspaceRel(entry.from, path); inside {
				if entry.images == nil {
					entry.images = make(map[string]workspaceImageChange)
				}
				entry.images[path] = change
			}
		}
	}
	a.treeEntriesChanged(moves, changes, true)
	return warning
}

func (a *App) moveTreeInto(node, dir *workspaceNode) error {
	if !canMoveTreeEntry(node, dir) {
		return errors.New("请选择另一个目标文件夹")
	}
	if err := a.checkTreeNode(node); err != nil {
		return err
	}
	if err := a.checkTreeNode(dir); err != nil {
		return err
	}
	to := filepath.Join(dir.path, node.name())
	return a.moveTreeEntry(node, to, "已移动 "+node.name())
}

func (a *App) duplicateTreeEntry(node *workspaceNode) error {
	if err := a.checkTreeNode(node); err != nil {
		return err
	}
	if sameWorkspacePath(node.path, a.root) {
		return errors.New("请选择要复制的文件或子文件夹")
	}
	to, err := duplicateWorkspacePath(node)
	if err != nil {
		return err
	}
	a.treeUndo = workspaceUndo{copies: workspaceCopiedEntries([]string{to})}
	a.refreshFiles()
	a.focusTreeEntry(to)
	a.status = "已复制为 " + filepath.Base(to)
	return nil
}

func duplicateWorkspacePath(node *workspaceNode) (string, error) {
	ext := filepath.Ext(node.name())
	if node.isDir() {
		ext = ""
	}
	stem := strings.TrimSuffix(node.name(), ext) + " 副本"
	for n := 1; ; n++ {
		name := stem + ext
		if n > 1 {
			name = fmt.Sprintf("%s %d%s", stem, n, ext)
		}
		to := filepath.Join(filepath.Dir(node.path), name)
		if _, err := os.Lstat(to); err == nil {
			continue
		} else if !errors.Is(err, fs.ErrNotExist) {
			return "", err
		}
		if err := copyWorkspacePath(node, to); err != nil {
			return "", err
		}
		return to, nil
	}
}

// Stage the copy next to its final path so an interrupted copy never leaves
// a partial document at the visible destination.
func copyWorkspacePath(node *workspaceNode, to string) error {
	if _, err := os.Lstat(to); err == nil {
		return &os.PathError{Op: "copy", Path: to, Err: fs.ErrExist}
	} else if !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	if node.isDir() {
		source, err := filepath.EvalSymlinks(node.path)
		if err != nil {
			return err
		}
		parent, err := filepath.EvalSymlinks(filepath.Dir(to))
		if err != nil {
			return err
		}
		if _, nested := workspaceRel(source, parent); nested {
			return errors.New("无法把文件夹复制到它自己的子目录")
		}
	}
	stage, err := os.MkdirTemp(filepath.Dir(to), ".markdownx-copy-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(stage)
	staged := filepath.Join(stage, "copy")
	if node.isDir() {
		err = copyWorkspaceDirectory(node, staged)
	} else {
		err = copyWorkspaceFile(node, staged)
	}
	if err != nil {
		return err
	}
	return moveWorkspacePath(staged, to)
}

func copyWorkspaceDirectory(node *workspaceNode, to string) error {
	root, err := os.OpenRoot(node.path)
	if err != nil {
		return err
	}
	defer root.Close()
	info, err := root.Stat(".")
	if err != nil {
		return err
	}
	if !os.SameFile(node.info, info) {
		return errors.New("源文件夹已变化")
	}
	return os.CopyFS(to, root.FS())
}

func copyWorkspaceFile(node *workspaceNode, to string) error {
	in, err := os.Open(node.path)
	if err != nil {
		return err
	}
	defer in.Close()
	info, err := in.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || !os.SameFile(node.info, info) {
		return errors.New("源文件已变化")
	}
	out, err := os.OpenFile(to, os.O_WRONLY|os.O_CREATE|os.O_EXCL, info.Mode().Perm())
	if err != nil {
		return err
	}
	_, writeErr := io.Copy(out, in)
	return errors.Join(writeErr, out.Close())
}

func (a *App) trashTreeEntry(node *workspaceNode) error {
	if sameWorkspacePath(node.path, a.root) {
		return errors.New("无法移除当前工作区的根目录")
	}
	if err := a.checkTreeNode(node); err != nil {
		return err
	}
	parent, err := os.Stat(filepath.Dir(node.path))
	if err != nil {
		return err
	}
	var trashed string
	if runtime.GOOS == "darwin" {
		trashed, err = mygo.Shell.TrashItemResult(node.path)
	} else {
		err = mygo.Shell.TrashItem(node.path)
	}
	if err != nil {
		return err
	}
	a.treeEntriesChanged(workspacePathMoves{{from: node.path}}, nil, false)
	a.treeUndo = workspaceUndo{}
	if trashed != "" {
		a.treeUndo.moves = []workspaceMoveUndo{{from: trashed, to: node.path, info: node.info, parent: parent, fromTrash: true}}
	}
	a.focusTreeEntry(filepath.Dir(node.path))
	a.status = "已移到" + workspaceTrashLabel() + " " + node.name()
	return nil
}

func (a *App) undoTreeOperation() error {
	undo := a.treeUndo
	if !undo.available() {
		return nil
	}
	for _, entry := range undo.moves {
		if err := a.checkWorkspaceMoveUndo(entry); err != nil {
			return fmt.Errorf("%s: %w", filepath.Base(entry.to), err)
		}
	}
	var remaining workspaceUndo
	var restored []workspaceMoveUndo
	var problem error
	for _, entry := range undo.moves {
		if err := moveWorkspacePath(entry.from, entry.to); err != nil {
			remaining.moves = append(remaining.moves, entry)
			problem = errors.Join(problem, err)
		} else {
			restored = append(restored, entry)
		}
	}
	status := "已撤销文件操作"
	var warning error
	if len(restored) > 0 {
		warning = a.restoreWorkspaceMovePaths(restored, len(remaining.moves) == 0)
		a.focusTreeEntry(restored[len(restored)-1].to)
		for _, entry := range remaining.moves {
			for path, change := range entry.images {
				// A retry can restore only the rest of the original batch.
				change.after = nil
				entry.images[path] = change
			}
		}
	}
	for _, copied := range undo.copies {
		if err := a.undoWorkspaceCopy(copied); err != nil {
			remaining.copies = append(remaining.copies, copied)
			problem = errors.Join(problem, fmt.Errorf("%s: %w", filepath.Base(copied.path), err))
		}
	}
	a.treeUndo = remaining
	if len(undo.moves) == 0 && len(undo.copies) > 0 {
		status = "已撤销复制"
	}
	a.reportWorkspaceImageChanges(status, warning)
	return problem
}

func (a *App) checkWorkspaceMoveUndo(undo workspaceMoveUndo) error {
	// Both the original parent and the item must still be the ones used
	// by the operation. The actual rename also refuses an occupied path.
	info, err := os.Lstat(undo.from)
	if err != nil {
		return err
	}
	if !os.SameFile(undo.info, info) {
		return errors.New("原条目已被替换")
	}
	rel, inside := workspaceRel(a.root, filepath.Dir(undo.to))
	if !inside {
		return errors.New("原位置已不在当前工作区")
	}
	base, err := filepath.EvalSymlinks(a.root)
	if err != nil {
		return err
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(undo.to))
	if err != nil {
		return err
	}
	parentInfo, err := os.Stat(parent)
	if err != nil {
		return err
	}
	if !sameWorkspacePath(parent, filepath.Join(base, rel)) || !os.SameFile(undo.parent, parentInfo) {
		return errors.New("原位置已变化")
	}
	if existing, err := os.Lstat(undo.to); err == nil {
		if !strings.EqualFold(undo.from, undo.to) || !os.SameFile(existing, info) {
			return &os.PathError{Op: "rename", Path: undo.to, Err: fs.ErrExist}
		}
	} else if !errors.Is(err, fs.ErrNotExist) {
		return err
	}
	return nil
}

func (a *App) restoreWorkspaceMovePaths(restored []workspaceMoveUndo, complete bool) error {
	moves := make(workspacePathMoves, len(restored))
	snapshots := make(map[string]workspaceImageChange)
	for i, entry := range restored {
		moves[i] = workspacePathMove{from: entry.from, to: entry.to}
		for path, change := range entry.images {
			if !complete {
				// Calculate paths from the entries that were actually restored.
				change.after = nil
			}
			snapshots[path] = change
		}
	}
	rebase := !restored[0].fromTrash
	var changes map[string]workspaceImageChange
	var warning error
	if rebase {
		changes, warning = a.rebaseWorkspaceImages(moves, snapshots)
	}
	a.treeEntriesChanged(moves, changes, rebase)
	return warning
}

type workspaceCopy struct {
	path        string
	info        fs.FileInfo
	fingerprint [sha256.Size]byte
}

func workspaceCopyFingerprint(path string) ([sha256.Size]byte, error) {
	h := sha256.New()
	err := filepath.WalkDir(path, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		fmt.Fprintf(h, "%q %d %d %d\n", p, info.Size(), info.Mode(), info.ModTime().UnixNano())
		return nil
	})
	var sum [sha256.Size]byte
	copy(sum[:], h.Sum(nil))
	return sum, err
}

func workspaceCopiedEntries(paths []string) []workspaceCopy {
	var copied []workspaceCopy
	for _, path := range paths {
		info, err := os.Lstat(path)
		if err != nil || !os.SameFile(info, info) {
			continue
		}
		fingerprint, err := workspaceCopyFingerprint(path)
		if err == nil {
			copied = append(copied, workspaceCopy{path: path, info: info, fingerprint: fingerprint})
		}
	}
	return copied
}

func (a *App) undoWorkspaceCopy(copied workspaceCopy) error {
	node := &workspaceNode{path: copied.path, info: copied.info}
	if err := a.checkTreeNode(node); err != nil {
		return err
	}
	fingerprint, err := workspaceCopyFingerprint(copied.path)
	if err != nil {
		return err
	}
	if fingerprint != copied.fingerprint {
		return errors.New("副本已有更改")
	}
	for _, w := range a.workspaceWindows() {
		if _, inside := workspaceRel(copied.path, w.path); inside {
			w.syncAll()
			w.updateDirty()
			if w.dirty {
				return errors.New("副本中有未保存的编辑")
			}
		}
	}
	if err := mygo.Shell.TrashItem(copied.path); err != nil {
		return err
	}
	a.treeEntriesChanged(workspacePathMoves{{from: copied.path}}, nil, false)
	a.focusTreeEntry(filepath.Dir(copied.path))
	return nil
}

func (a *App) workspaceWindows() []*App {
	if a.windows != nil {
		return a.windows.order
	}
	return []*App{a}
}

// A successful Markdown save replaces the file. Transfer its tree undo only
// when the pre-save identity still matches the file moved by MarkdownX.
func (a *App) workspaceUndosForSave(path string) []*workspaceMoveUndo {
	var matches []*workspaceMoveUndo
	var info fs.FileInfo
	for _, w := range a.workspaceWindows() {
		for i := range w.treeUndo.moves {
			undo := &w.treeUndo.moves[i]
			if !sameWorkspacePath(undo.from, path) {
				if undo.images[path].failed {
					matches = append(matches, undo)
				}
				continue
			}
			if info == nil {
				var err error
				info, err = os.Lstat(path)
				if err != nil {
					return nil
				}
			}
			if os.SameFile(undo.info, info) {
				matches = append(matches, undo)
			}
		}
	}
	return matches
}

// Remap paths while retaining every open editor's unsaved changes.
func (a *App) treeEntriesChanged(moves workspacePathMoves, images map[string]workspaceImageChange, rebase bool) {
	repath := moves.path
	if a.windows != nil {
		a.windows.closed = remapRecentPaths(a.windows.closed, repath)
	}
	s := loadSettings()
	s.Root, s.Last = repath(s.Root), repath(s.Last)
	s.Recent = remapRecentPaths(s.Recent, repath)
	if rebase || moves[0].to != "" {
		s.DefaultWorkspace = repath(s.DefaultWorkspace)
		s.PinnedFolders = remapRecentPaths(s.PinnedFolders, repath)
		s.RecentFolders = remapRecentPaths(s.RecentFolders, repath)
	}
	for _, w := range a.workspaceWindows() {
		for tab := range w.openDocuments {
			if path := repath(tab.path); path != tab.path {
				w.withDocumentPane(tab, func() { w.remapOpenDocument(moves, path, images[path], rebase) })
			}
		}
		w.remapClosedTabs(moves)
		root, rootMove := moves.remap(w.root)
		if root == "" && w.root != "" {
			root = filepath.Dir(rootMove.from)
		}
		affected := slices.ContainsFunc(moves, func(move workspacePathMove) bool {
			_, lost := workspaceRel(w.root, move.from)
			_, gained := workspaceRel(w.root, move.to)
			return lost || gained
		})
		if root != w.root || affected {
			var nextOpen ui.Selection[string]
			if w.treeView != nil {
				for path := range w.treeView.Open.All() {
					if p := repath(path); p != "" {
						nextOpen.Add(p)
					}
				}
			}
			if root != w.root || w.treeView == nil {
				nextOpen.Add(root)
			}
			w.root = root
			w.refreshFiles()
			if w.treeView != nil {
				w.treeView.Open = nextOpen
				w.treeView.keepPath = repath(w.treeView.keepPath)
				w.revealWorkspaceFile()
			}
		}
		w.recents = slices.Clone(s.Recent)
		w.services.Invalidate()
	}
	saveSettings(s)
}

func (a *App) remapOpenDocument(moves workspacePathMoves, path string, images workspaceImageChange, rebase bool) {
	a.syncAll()
	a.updateDirty()
	if path == "" {
		// A modified document remains available as an unsaved copy.
		if !a.dirty {
			a.resetDocument(doc.NewDocument())
		}
		a.file = markdownFile{}
	} else if a.file.path != "" {
		if rebase && !sameWorkspacePath(filepath.Dir(a.path), filepath.Dir(path)) {
			a.rebaseWorkspaceEditor(moves, path, images)
		} else {
			a.file.path = path
		}
	}
	a.path = path
	a.resetPreview()
}

func workspaceTrashLabel() string {
	if runtime.GOOS == "windows" {
		return "回收站"
	}
	return "废纸篓"
}

func remapRecentPaths(paths []string, repath func(string) string) []string {
	var list []string
	for _, p := range paths {
		if next := repath(p); next != "" && !slices.Contains(list, next) {
			list = append(list, next)
		}
	}
	return list
}
