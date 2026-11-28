package app

import (
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/editor"
)

func testDocumentWindows(from, to *App) *desktopWindows {
	d := &desktopWindows{order: []*App{from, to}, active: from}
	from.windows, to.windows = d, d
	return d
}

func TestNativeTabTransferWaitsForDropAndKeepsLiveDraft(t *testing.T) {
	config := isolateConfig(t)
	fromFolder, toFolder := t.TempDir(), t.TempDir()
	fromPath := tabTestFile(t, fromFolder, "原稿.md", utf8BOM+"原文\r\n")
	toPath := tabTestFile(t, toFolder, "目标窗口.md", "目标\n")
	from, to := New(fromPath), New(toPath)
	desktop := testDocumentWindows(from, to)
	original := from.documentTab
	from.enterSourceAt(from.markdownSource(), len("原文"))
	from.source.edit.Insert("，草稿")
	from.updateDirty()
	selection := from.source.edit.Selection()

	drag := &documentDrag{from: from, tab: original, native: true}
	drop := func() { to.receiveDocumentDrag(drag, false, 0) }
	drag.onDrop(drop)
	if len(from.tabs) != 1 || len(to.tabs) != 1 {
		t.Fatal("a destination frame moved the tab before the native drag finished")
	}
	drag.finish(false, transfer.Result{Operation: transfer.Copy})
	drag.onDrop(drop)
	if len(from.tabs) != 0 || len(desktop.order) != 1 || desktop.active != to || len(desktop.closed) != 0 {
		t.Fatal("moving the last tab did not retire the source without adding a closed file")
	}
	if to.root != toFolder || len(to.tabs) != 2 || to.tabs[0] != original || to.documentTab != original {
		t.Fatal("the destination did not preserve its own workspace and receive the live tab at the requested slot")
	}
	if !to.dirty || to.source.edit.Selection() != selection || to.markdownSource() != "原文，草稿\n" {
		t.Fatal("the transfer lost the unsaved source text or selection")
	}
	tr := ui.NewTester(to.View, 1120, 680)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyZ)
	if to.dirty || to.markdownSource() != "原文\n" {
		t.Fatal("undo no longer reached the editing history from the source window")
	}
	tr.Key(ui.Cmd, ui.KeyY)
	tr.Key(ui.Cmd, ui.KeyS)
	if got, err := os.ReadFile(fromPath); err != nil || string(got) != utf8BOM+"原文，草稿\r\n" {
		t.Fatalf("saving the transferred document = %q, %v", got, err)
	}
	for _, dir := range []string{config, toFolder} {
		if _, err := os.Stat(filepath.Join(dir, "原稿.md")); !os.IsNotExist(err) {
			t.Fatalf("moving an editor created a file in %q: %v", dir, err)
		}
	}
}

func TestDragCancelAndPathConflictLeaveBothWindowsUntouched(t *testing.T) {
	root := isolateConfig(t)
	conflict := tabTestFile(t, root, "共用.md", "原始\n")
	second := tabTestFile(t, root, "另一栏.md", "另一栏\n")
	from, to := New(conflict), New(conflict)
	desktop := testDocumentWindows(from, to)
	from.openDocumentInSplit(second, -1)
	group := from.documentTab.tabOwner()
	active := from.documentTab

	drag := &documentDrag{from: from, tab: group, native: true}
	drag.onDrop(func() { t.Fatal("an Esc-canceled drag delivered a tab") })
	drag.finish(false, transfer.Result{Canceled: true, Escaped: true})
	drag.onDrop(func() { t.Fatal("a late drop after Escape delivered a tab") })
	if from.documentTab != active || len(from.tabs) != 1 || len(to.tabs) != 1 {
		t.Fatal("Escape changed one of the tab strips")
	}

	drag = &documentDrag{from: from, tab: group, native: true}
	drag.finish(false, transfer.Result{Operation: transfer.Copy})
	drag.onDrop(func() { to.receiveDocumentDrag(drag, false, 1) })
	if len(desktop.order) != 2 || len(from.tabs) != 1 || len(to.tabs) != 1 ||
		from.documentTab != active || from.status == "" || to.status == "" {
		t.Fatal("an independently loaded destination conflict did not reject the whole split atomically")
	}
}

func TestSplitTabMoveKeepsOrderWidthAndActivePane(t *testing.T) {
	root := isolateConfig(t)
	leftPath := tabTestFile(t, root, "左.md", "左\n")
	rightPath := tabTestFile(t, root, "右.md", "右\n")
	targetPath := tabTestFile(t, root, "窗口.md", "目标\n")
	from, to := New(leftPath), New(targetPath)
	desktop := testDocumentWindows(from, to)
	from.openDocumentInSplit(rightPath, -1)
	group, active := from.documentTab.tabOwner(), from.documentTab
	layout := from.split
	left, right := layout.panes[0], layout.panes[1]
	left.splitWeight, right.splitWeight = 1.4, .6
	right.editScroll.Y = 91
	from.newTab()
	remaining := from.documentTab
	target := to.documentTab
	drag := &documentDrag{from: from, tab: group, native: true}
	drag.finish(false, transfer.Result{Operation: transfer.Copy})
	drag.onDrop(func() { to.receiveDocumentDrag(drag, false, 1) })
	if len(desktop.order) != 2 || from.documentTab != remaining || len(from.tabs) != 1 ||
		!slices.Equal(to.tabs, []*documentTab{target, group}) || to.documentTab != active || to.split != layout {
		t.Fatal("moving a background split did not transfer its active pane and preserve both tab strips")
	}
	if !slices.Equal(layout.panes, []*documentTab{left, right}) ||
		left.splitWeight != 1.4 || right.splitWeight != .6 || right.editScroll.Y != 91 {
		t.Fatal("the transferred split did not keep its layout or scroll state")
	}
}

func TestMovedSplitPaneSharesDocumentAndKeepsWindowSelections(t *testing.T) {
	root := isolateConfig(t)
	path := tabTestFile(t, root, "同步.md", "前段 后段\n")
	from, to := New(path), New(tabTestFile(t, root, "右侧.md", "另一篇\n"))
	testDocumentWindows(from, to)
	left := from.documentTab
	block := left.core.Doc.Blocks[0]
	from.focusAt(editor.Caret(editor.Pos{Block: block.ID, Offset: len("前段 ")}))
	trLeft := ui.NewTester(from.View, 1120, 680)
	trLeft.Frame()
	from.splitRight()
	right := from.documentTab
	trLeft.Frame()
	to.receiveSplitPaneDrag(&documentDrag{from: from, tab: right}, nil, 1)
	if from.documentTab != left || from.split != nil || to.documentTab != right ||
		right.documentState != left.documentState || len(to.split.panes) != 2 {
		t.Fatal("the moved pane was not inserted beside the target's document")
	}
	trRight := ui.NewTester(to.View, 1120, 680)
	trRight.Frame()
	trLeft.Frame() // Let the other window render and claim the shared core before typing.
	trRight.Key(0, ui.KeyEnd)
	trRight.Type(" 新字")
	trLeft.Frame()
	if got := from.core.Sel.Head.Offset; got != len("前段 ") {
		t.Fatalf("the receiving window moved the other window's caret to byte %d", got)
	}
	trLeft.Type("中间 ")
	if to.markdownSource() != "前段 中间 后段 新字\n" {
		t.Fatalf("cross-window editing did not update one document: %q", to.markdownSource())
	}
	trLeft.Key(ui.Cmd, ui.KeySlash)
	if left.source == nil || right.source == nil || left.source.edit.Buffer != right.source.edit.Buffer {
		t.Fatal("switching to source did not update the two views over a common buffer")
	}
	trRight.Frame()
	trRight.Key(ui.Cmd, ui.KeySlash)
	if left.source != nil || right.source != nil || to.markdownSource() != from.markdownSource() {
		t.Fatal("closing the source projection left an obsolete view in the other window")
	}
	if !from.confirmWindowClose() || !to.dirty {
		t.Fatal("closing one view prompted to discard a draft still open in another window")
	}
}

func TestDetachedWindowRespectsNegativeMonitorOrigin(t *testing.T) {
	area := mygo.Rectangle{X: -840, Y: 30, Width: 840, Height: 680}
	bounds := detachedWindowBounds(mygo.Point{X: -30, Y: 630}, area)
	if bounds.X != -840 || bounds.Y != 30 || bounds.Width != 840 || bounds.Height != 680 {
		t.Fatalf("unexpected work-area placement: %+v", bounds)
	}
}
