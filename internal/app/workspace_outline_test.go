package app

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeDragScrollsToAnOffscreenFolder(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	source := filepath.Join(root, "000 源", "记录.md")
	for i := range 40 {
		dir := filepath.Join(root, fmt.Sprintf("%03d 目标", i))
		if i == 0 {
			dir = filepath.Dir(source)
		}
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(source, []byte("# 记录\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	a.OpenFile(source)
	tr := ui.NewTester(a.View, 960, 620)
	tr.Frame()
	box, ok := tr.Find("文件目录树")
	if !ok {
		t.Fatal("missing tree viewport")
	}
	startDrag := func() (float32, float32) {
		t.Helper()
		row, ok := workspaceTreeRow(tr, "记录.md")
		if !ok || tr.HasText("039 目标") {
			t.Fatal("test requires a visible source and offscreen destination")
		}
		tr.Press(row.X+row.W/2, row.Y+row.H/2)
		tr.Move(box.X+box.W/2, box.Y+box.H-4)
		for range 150 {
			viewport, _ := tr.Find("文件目录树")
			if target, ok := workspaceTreeRow(tr, "039 目标"); ok && target.Y >= viewport.Y && target.Y+target.H <= viewport.Y+viewport.H {
				x, y := target.X+target.W/2, target.Y+target.H/2
				tr.Move(x, y)
				return x, y
			}
			tr.Advance(time.Second / 60)
		}
		t.Fatal("resting near the bottom during a drag did not reveal the offscreen folder")
		return 0, 0
	}
	x, y := startDrag()
	tr.Key(0, ui.KeyEscape)
	tr.Release(x, y)
	if _, err := os.Stat(source); err != nil || a.treeUndo.available() {
		t.Fatalf("canceled drag changed the filesystem: %v", err)
	}
	a.treeView.List.ScrollTo(0, ui.Start)
	tr.Frame()
	x, y = startDrag()
	tr.Release(x, y)
	dest := filepath.Join(root, "039 目标", "记录.md")
	if data, err := os.ReadFile(dest); err != nil || string(data) != "# 记录\n" || !samePath(a.path, dest) || a.treeEdit.open {
		t.Fatalf("drop after scrolling did not move the active file: %q (%v)", a.path, err)
	}
}

func TestFileTreeUsesTheWholeRowForMenusAndDrops(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "工作区")
	chapter := filepath.Join(root, "专题")
	destination := filepath.Join(root, "归档")
	for _, path := range []string{chapter, destination} {
		if err := os.MkdirAll(path, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	note := filepath.Join(chapter, "记录.md")
	if err := os.WriteFile(note, []byte("# 记录\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	box, ok := tr.Find("文件目录树")
	if !ok {
		t.Fatal("the workspace tree was missing")
	}
	row, ok := tr.Find("专题")
	if !ok {
		t.Fatal("the collapsed folder row was missing")
	}
	y := row.Y + row.H/2
	indentX, arrowX := box.X+8, box.X+workspaceStartIndent+workspaceLevelIndent+workspaceArrowWidth/2
	for _, x := range []float32{indentX, arrowX, box.X + box.W - 12} {
		tr.RightClickAt(x, y)
		if err := tr.ChooseMenuItem("复制路径"); err != nil {
			t.Fatalf("folder context at X=%v: %v", x, err)
		}
		if tr.Clipboard() != chapter || a.treeView.Open.Has(chapter) || a.path != "" {
			t.Fatalf("secondary click at X=%v acted on a different row: %q", x, tr.Clipboard())
		}
	}
	tr.ClickAt(arrowX, y)
	if !a.treeView.Open.Has(chapter) || !tr.HasText("记录.md") {
		t.Fatal("primary click on the arrow did not expand the folder once")
	}
	fileRow, _ := workspaceTreeRow(tr, "记录.md")
	tr.ClickAt(indentX, fileRow.Y+fileRow.H/2)
	if a.path != note || !workspaceTestFocused(a, tr, "记录.md") {
		t.Fatal("primary click on a file's indentation did not open and select the file")
	}
	fileRow, _ = workspaceTreeRow(tr, "记录.md")
	archiveRow, _ := tr.Find("归档")
	x, y := fileRow.X+fileRow.W/2, fileRow.Y+fileRow.H/2
	dropY := archiveRow.Y + archiveRow.H/2
	tr.Press(x, y)
	tr.Move(indentX, dropY)
	tr.Release(indentX, dropY)
	if a.treeEdit.open || a.path != filepath.Join(destination, "记录.md") {
		t.Fatal("dropping on the indentation did not target its row's folder")
	}
	tr.RightClickAt(indentX, box.Y+box.H-20)
	if err := tr.ChooseMenuItem("复制路径"); err != nil || tr.Clipboard() != root {
		t.Fatalf("the blank tree area did not open the root context: %q, %v", tr.Clipboard(), err)
	}
}

func TestFileTreeMenuKeyRevealsAnOffscreenSelection(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "工作区")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	const total = 110
	for i := range total {
		if err := os.WriteFile(filepath.Join(root, fmt.Sprintf("记录 %03d.md", i)), nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	last := fmt.Sprintf("记录 %03d.md", total-1)
	tr.Key(0, ui.KeyEnd)
	if !workspaceTestFocused(a, tr, last) || !tr.HasText(last) {
		t.Fatal("the initial keyboard selection was not at the end of the tree")
	}
	box, ok := tr.Find("文件目录树")
	if !ok {
		t.Fatal("the workspace tree was missing")
	}
	tr.Scroll(box.X+box.W/2, box.Y+box.H/2, 0, -float32(total*30))
	if tr.HasText(last) || !workspaceTestFocused(a, tr, last) {
		t.Fatal("scrolling did not take the selected row outside the viewport")
	}
	tr.Key(ui.Shift, ui.KeyF10)
	if !tr.HasText(last) {
		t.Fatal("Shift+F10 did not bring the selected row into view")
	}
	if err := tr.ChooseMenuItem("复制路径"); err != nil || tr.Clipboard() != filepath.Join(root, last) {
		t.Fatalf("the menu did not act on the selected row: %q, %v", tr.Clipboard(), err)
	}
	if a.path != "" || !workspaceTestFocused(a, tr, last) {
		t.Fatal("closing the context menu changed the active document or lost keyboard focus")
	}
	tr.Scroll(box.X+box.W/2, box.Y+box.H/2, 0, -float32(total*30))
	tr.Key(0, ui.KeyContextMenu)
	if !tr.HasText(last) || len(tr.Menu()) == 0 {
		t.Fatal("the dedicated menu key did not reveal the same row")
	}
	tr.Key(0, ui.KeyEscape)
	if len(tr.Menu()) != 0 || !workspaceTestFocused(a, tr, last) {
		t.Fatal("Escape did not dismiss the menu and return to the selected row")
	}
}

func TestFileTreeDeepLabelsAndArrowsFitTheNarrowSidebar(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "工作区")
	deep := root
	for i := range 16 {
		deep = filepath.Join(deep, fmt.Sprintf("层级%02d", i+1))
	}
	if err := os.MkdirAll(deep, 0o700); err != nil {
		t.Fatal(err)
	}
	const name = "深层项目的完整记录.markdown"
	note := filepath.Join(deep, name)
	if err := os.WriteFile(note, []byte("# 深层记录\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	a.sidebarWidth = minSidebarWidth
	a.OpenFile(note)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(0, ui.KeyHome)
	if !workspaceTestFocused(a, tr, "工作区") || !a.treeView.Open.Has(deep) || !workspaceHasTreeRow(tr, name) {
		t.Fatal("the deep branch was not revealed alongside the root selection")
	}
	tree, ok := tr.Find("文件目录树")
	row, rowOK := tr.Find(filepath.Base(deep))
	earlier, earlierOK := tr.Find("层级08")
	if !ok || !rowOK || !earlierOK {
		t.Fatal("the narrow tree or its deepest folder was missing")
	}
	x := tree.X + tree.W - workspaceRowPadding - workspaceMinLabelWidth - workspaceIconWidth - workspaceArrowWidth/2
	y := row.Y + row.H/2
	tr.ClickAt(x, y)
	if a.treeView.Open.Has(deep) || workspaceHasTreeRow(tr, name) || !workspaceTestFocused(a, tr, "工作区") {
		t.Fatal("the deepest arrow was clipped or acted as a click on the row")
	}
	tr.ClickAt(x, y)
	if !a.treeView.Open.Has(deep) || !workspaceHasTreeRow(tr, name) || a.path != note {
		t.Fatal("the deep branch could not be reopened without changing the document")
	}
	// At roughly half the depth, the arrow should be farther left than the deepest one.
	left := tree.X + (x-tree.X+workspaceStartIndent+workspaceArrowWidth/2)/2
	y = earlier.Y + earlier.H/2
	tr.ClickAt(left, y)
	if workspaceHasTreeRow(tr, name) || !workspaceTestFocused(a, tr, "工作区") {
		t.Fatal("the ancestor arrow did not remain at its own indentation")
	}
	tr.ClickAt(left, y)
	if !workspaceHasTreeRow(tr, name) || !workspaceTestFocused(a, tr, "工作区") {
		t.Fatal("reopening the ancestor lost the descendant's expanded state")
	}
	saveWorkspaceSnapshot(t, tr, "26-workspace-narrow-branches")
}
