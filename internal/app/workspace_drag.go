package app

import (
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
)

var workspaceFileDrop = transfer.DropOptions{Formats: []transfer.Format{transfer.FileList, transfer.URIList}, Operations: transfer.Copy}

var workspacePathEscapes = strings.NewReplacer("&", "%26", "(", "%28", ")", "%29")

func workspaceMarkdownPath(relative string) string {
	return workspacePathEscapes.Replace((&url.URL{Path: filepath.ToSlash(relative)}).EscapedPath())
}

func (a *App) dragTreeEntry(row ui.Element, node *workspaceNode) {
	row.Drag(node)
	a.treePointerHeld = a.treePointerHeld || row.Pressed() || row.Dragging()
	if a.win != nil {
		row.DragDataFrom(func() transfer.Data {
			if err := a.checkTreeNode(node); err != nil {
				a.treeActionStatus(err)
				return transfer.Data{}
			}
			data, err := transfer.FileData(node.path)
			a.treeActionStatus(err)
			return data
		}, transfer.DragOptions{Operations: transfer.Copy, Done: func(result transfer.Result) {
			a.treeActionStatus(result.Err)
		}})
	}
}

func (a *App) dropOnTreeDirectory(c *ui.Context, row ui.Element, dir *workspaceNode, expand bool) {
	offer, fileOver := ui.DataDragOver(row, workspaceFileDrop)
	source, localOver := ui.DragOver[*workspaceNode](row)
	copyDrop := c.Modifiers()&ui.Alt != 0
	if a.win != nil && (localOver || fileOver) {
		copyDrop = copyDrop || offer.Suggested == transfer.Copy || workspaceNativeAltPressed()
	}
	canDrop, hint := fileOver || row.FileDragOver(), "放入此处"
	if localOver {
		_, inside := workspaceRel(a.root, source.path)
		copyItem := copyDrop || !inside
		canDrop = canMoveTreeEntry(source, dir) || copyItem && sameWorkspacePath(filepath.Dir(source.path), dir.path)
		hint = "移动到此处"
		if copyItem {
			hint = "复制到此处"
		}
	}
	if canDrop {
		row.Background(c.Theme().Selection)
		showWorkspaceDropHint(c, hint)
		a.treeDropSeen = true
		if a.treeDropTarget != dir.path {
			a.treeDropTarget = dir.path
			a.treeDropExpand = c.Now().Add(600 * time.Millisecond)
		}
		a.treeDropCopy = copyDrop
		if expand && !a.treeView.Open.Has(dir.path) {
			if wait := a.treeDropExpand.Sub(c.Now()); wait > 0 {
				c.After(wait)
			} else {
				a.treeView.Open.Add(dir.path)
				a.services.Invalidate()
			}
		}
	}

	// The drop frame may have lost the modifier offer from the last
	// drag-over frame. Keep that choice only for its matching folder.
	copyDrop = copyDrop || a.treeDropTarget == dir.path && a.treeDropCopy
	if source, ok := ui.Drop[*workspaceNode](row); ok {
		a.dropWorkspaceTreeEntry(source, dir, copyDrop)
		return
	}
	if drop, ok := ui.DropData(row, workspaceFileDrop); ok {
		paths, err := drop.Data.Files()
		a.treeActionStatus(err)
		a.routeWorkspaceDrop(dir, paths, copyDrop)
		return
	}
	a.routeWorkspaceDrop(dir, row.DroppedFiles(), copyDrop)
}

func (a *App) dropWorkspaceTreeEntry(source, dir *workspaceNode, copyDrop bool) {
	_, inside := workspaceRel(a.root, source.path)
	if inside && !copyDrop {
		if canMoveTreeEntry(source, dir) {
			a.treeActionStatus(a.moveTreeInto(source, dir))
		}
		return
	}
	if err := a.checkDraggedTreeEntry(source); err != nil {
		a.treeActionStatus(err)
		return
	}
	a.routeWorkspaceDrop(dir, []string{source.path}, true)
}

func (a *App) checkDraggedTreeEntry(source *workspaceNode) error {
	if _, inside := workspaceRel(a.root, source.path); inside {
		return a.checkTreeNode(source)
	}
	// A typed drag from another window still refers to its original entry.
	info, err := os.Lstat(source.path)
	if err != nil {
		return err
	}
	if !os.SameFile(source.info, info) {
		return errWorkspaceEntryChanged
	}
	return nil
}

func showWorkspaceDropHint(c *ui.Context, label string) {
	root := c.Root()
	px, py, _ := root.PointerPosition()
	theme := c.Theme()
	root.DrawOver(func(p *ui.Painter, r ui.Rect) {
		span := ui.Span{Text: label, Font: theme.Font, Size: 12, Weight: 600, Color: theme.AccentText}
		w, h := p.MeasureText(0, span)
		const inset, gap = 6, 8
		x := min(max(px+gap, 0), max(0, r.W-w-inset*2)) + r.X
		y := py + gap
		if py >= h+gap+inset {
			y = py - h - gap - inset
		}
		p.Fill(ui.Rect{X: x, Y: r.Y + y, W: w + inset*2, H: h + inset}, theme.Accent, theme.Radius)
		p.RichText(x+inset, r.Y+y+inset/2, 0, span)
	})
}

func (a *App) dropEmptyWorkspace(c *ui.Context, pane ui.Element) {
	_, over := ui.DataDragOver(pane, workspaceFileDrop)
	if over || pane.FileDragOver() {
		pane.Background(c.Theme().Selection)
		showWorkspaceDropHint(c, "打开工作区")
	}
	paths := pane.DroppedFiles()
	if drop, ok := ui.DropData(pane, workspaceFileDrop); ok {
		var err error
		paths, err = drop.Data.Files()
		a.treeActionStatus(err)
	}
	if len(paths) == 0 {
		return
	}
	path := paths[0]
	info, err := os.Stat(path)
	if err != nil {
		a.treeActionStatus(err)
		return
	}
	if info.IsDir() {
		a.openWorkspaceFolder(path)
		return
	}
	a.openWorkspaceFolder(filepath.Dir(path))
	a.focusTreeEntry(path)
}

func (a *App) workspaceRelativePath(path string) (string, bool) {
	base := a.root
	if a.path != "" {
		base = filepath.Dir(a.path)
	}
	if base == "" {
		return "", false
	}
	rel, err := filepath.Rel(base, path)
	return rel, err == nil
}
