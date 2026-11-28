package app

import (
	"os"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/editor"
)

func TestFileTreeOpenInSplitAndReopenKeepsGroup(t *testing.T) {
	root := isolateConfig(t)
	first := tabTestFile(t, root, "甲.md", "左边分栏\n")
	second := tabTestFile(t, root, "乙.md", "右边分栏\n")
	a := New(first)
	tr := ui.NewTester(a.View, 1120, 680)
	tr.Frame()
	row, ok := workspaceTreeRow(tr, "乙.md")
	if !ok {
		t.Fatal("missing file tree item")
	}
	tr.RightClickAt(row.X+row.W/2, row.Y+row.H/2)
	if err := tr.ChooseMenuItem("打开方式", "在分栏中打开"); err != nil {
		t.Fatal(err)
	}
	if a.path != second || len(a.tabs) != 1 || a.split == nil || len(a.split.panes) != 2 || a.split.panes[0].path != first {
		t.Fatalf("opening another document produced %d tabs, active %q, split %v", len(a.tabs), a.path, a.split != nil)
	}
	if !tr.HasText("甲.md | 乙.md") || !hasSplitEditorText(tr, "左边分栏") || !hasSplitEditorText(tr, "右边分栏") {
		t.Fatal("the grouped tab and its two documents are missing")
	}
	left := a.split.panes[0]
	for _, field := range tr.AccessAll() {
		if field.Role == ui.RoleTextField && field.Name == "正文" && field.Value == "左边分栏" {
			pos := field.Bounds
			tr.ClickAt(pos.X+10, pos.Y+pos.H/2)
			break
		}
	}
	tr.Key(0, ui.KeyEnd)
	tr.Type(" 左栏修改")
	tr.Key(ui.Cmd, ui.KeyS)
	if raw, err := os.ReadFile(first); err != nil || string(raw) != "左边分栏 左栏修改\n" || a.documentTab != left || a.split.panes[1].dirty {
		t.Fatalf("editing an inactive pane routed text or Save to the wrong document: %q, %v, %q", raw, err, a.path)
	}
	workspaceTestClick(t, tr, "聚焦分栏 甲.md")
	if a.documentTab != left || !workspaceHasTreeRow(tr, "甲.md") {
		t.Fatal("activating a split did not keep the workspace tree")
	}
	a.newTab()
	a.selectDocumentTab(left, true)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyW)
	if len(a.tabs) != 1 || a.split != nil || a.path != "" {
		t.Fatal("closing the grouped tab did not leave the other tab")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyT)
	if a.split == nil || a.path != first || len(a.split.panes) != 2 || a.split.panes[1].path != second {
		t.Fatal("reopen did not restore the previous layout and active pane")
	}
	workspaceTestClick(t, tr, "关闭分栏 乙.md")
	if a.split != nil || a.path != first || !hasSplitEditorText(tr, "左边分栏 左栏修改") {
		t.Fatal("closing one pane did not return the remaining document to an ordinary tab")
	}
}

func hasSplitEditorText(tr *ui.Tester, text string) bool {
	for _, target := range tr.AccessAll() {
		if target.Role == ui.RoleTextField && target.Name == "正文" && target.Value == text && !target.Offscreen {
			return true
		}
	}
	return false
}

func TestSplitSameDocumentSharesVisualAndSourceEdits(t *testing.T) {
	root := isolateConfig(t)
	original := utf8BOM + "开头 共同 结尾\r\n"
	path := tabTestFile(t, root, "共同.md", original)
	a := New(path)
	left := a.documentTab
	block := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: block.ID, Offset: len("开头 ")}))
	tr := ui.NewTester(a.View, 1120, 680)
	tr.Frame()
	tr.Key(ui.Cmd|ui.Alt, ui.KeyBackslash)
	if a.split == nil || len(a.split.panes) != 2 || len(a.tabs) != 1 || a.documentTab == left {
		t.Fatal("the split-right shortcut did not create a second pane")
	}
	right := a.documentTab
	if left.core != right.core || left.bufs[block.ID] == right.bufs[block.ID] {
		t.Fatal("the two panes must share one model and have independent text controls")
	}
	tr.Key(0, ui.KeyEnd)
	tr.Type(" 右")
	paneText := func(pane *documentTab) string { return pane.bufs[block.ID].native.Text().String() }
	if got := a.markdownSource(); got != "开头 共同 结尾 右\n" || paneText(left) != paneText(right) {
		t.Fatalf("edit in the right pane did not refresh both. source %q, left %q, right %q", got, paneText(left), paneText(right))
	}
	a.stepSplit(-1)
	tr.Frame()
	tr.Type("左注 ")
	if got := a.markdownSource(); got != "开头 左注 共同 结尾 右\n" {
		t.Fatalf("the original caret did not survive the other pane's edit: %q", got)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if got := a.markdownSource(); got != "开头 共同 结尾 右\n" || paneText(left) != paneText(right) {
		t.Fatalf("the left undo did not update both panes: %q", got)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if a.dirty || a.markdownSource() != "开头 共同 结尾\n" {
		t.Fatal("one shared history did not undo both visual edits")
	}
	a.focusAt(editor.Caret(editor.Pos{Block: block.ID, Offset: len("开头 共同 ")}))
	tr.Frame()
	a.stepSplit(1)
	tr.Frame()
	tr.Key(0, ui.KeyHome)
	tr.Type("前缀 ")
	a.stepSplit(-1)
	tr.Frame()
	if got := a.core.Sel.Head.Offset; got != len("前缀 开头 共同 ") {
		t.Fatalf("an insertion before the other caret left it at byte %d", got)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	tr.Key(ui.Cmd, ui.KeySlash)
	if left.source == nil || right.source == nil || left.source.edit.Buffer != right.source.edit.Buffer || left.source == right.source {
		t.Fatal("source mode did not provide separate views over a shared buffer")
	}
	left.source.edit.Select(0, 0)
	right.source.edit.Select(len("开头 共同 结尾"), len("开头 共同 结尾"))
	a.stepSplit(1)
	tr.Frame()
	tr.Type(" 源码")
	if left.source.text() != "开头 共同 结尾 源码\n" || left.source.edit.Selection().Head != 0 {
		t.Fatal("the source edit did not update both views or moved the other caret")
	}
	a.stepSplit(-1)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyS)
	if got, err := os.ReadFile(path); err != nil || string(got) != utf8BOM+"开头 共同 结尾 源码\r\n" || right.dirty {
		t.Fatalf("save from the other pane did not preserve the exact format: %q, %v", got, err)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if got := right.source.text(); got != "开头 共同 结尾\n" {
		t.Fatalf("undo in the other source pane left %q", got)
	}
	tr.Key(ui.Cmd, ui.KeySlash)
	if left.source != nil || right.source != nil || a.markdownSource() != "开头 共同 结尾\n" {
		t.Fatal("returning to visual mode did not commit one common source version")
	}
	tr.Key(ui.Cmd, ui.KeyS)
	if got, err := os.ReadFile(path); err != nil || string(got) != original {
		t.Fatalf("restoring the original text was lossy: %q, %v", got, err)
	}
}

func TestSplitSameDocumentKeepsSeparateScrollPositions(t *testing.T) {
	root := isolateConfig(t)
	text := strings.Repeat("同一篇文档的测试段落。\n\n", 60)
	a := New(tabTestFile(t, root, "长文.md", text))
	left := a.documentTab
	a.splitRight()
	right := a.documentTab
	tr := ui.NewTester(a.View, 1120, 680)
	tr.Frame()
	if left.editScroll.MaxY < 500 || right.editScroll.MaxY < 500 {
		t.Fatal("the fixture did not produce two scrollable visual panes")
	}
	left.editScroll.Y, right.editScroll.Y = 140, 430
	tr.Frame()
	a.stepSplit(-1)
	tr.Frame()
	a.stepSplit(1)
	tr.Frame()
	if left.editScroll.Y != 140 || right.editScroll.Y != 430 {
		t.Fatalf("switching panes changed the scroll positions: %v / %v", left.editScroll.Y, right.editScroll.Y)
	}
	tr.Key(ui.Cmd, ui.KeySlash)
	left.source.edit.SetScrollY(110)
	right.source.edit.SetScrollY(360)
	tr.Frame()
	a.stepSplit(-1)
	tr.Frame()
	a.stepSplit(1)
	tr.Frame()
	if left.source.edit.ScrollY() != 110 || right.source.edit.ScrollY() != 360 {
		t.Fatalf("switching panes changed the source scroll positions: %v / %v", left.source.edit.ScrollY(), right.source.edit.ScrollY())
	}
}

func TestReopenedDetachedDraftUsesCurrentSharedEditorMode(t *testing.T) {
	isolateConfig(t)
	a := New("")
	a.enterSourceAt("草稿正文\n", 0)
	a.updateDirty()
	left := a.documentTab
	a.splitRight()
	a.detachSplitPane(left, 1)
	a.closeActiveTab()
	if len(a.tabs) != 1 || a.documentTab == left || a.source == nil {
		t.Fatal("the other view of the draft did not remain open")
	}
	a.toggleSource()
	if !a.reopenDocumentTab() || a.source != nil || a.markdownSource() != "草稿正文\n" {
		t.Fatal("reopening a detached view restored stale source mode or lost the draft")
	}
	if a.tabs[0].documentState != a.tabs[1].documentState {
		t.Fatal("the reopened view no longer shares the existing draft")
	}
}

func TestSplitDropResizeReorderAndDetach(t *testing.T) {
	root := isolateConfig(t)
	first := tabTestFile(t, root, "甲.md", "First\n")
	second := tabTestFile(t, root, "乙.md", "Second\n")
	external := tabTestFile(t, t.TempDir(), "丙.md", "Third\n")
	a := New(first)
	active := a.documentTab
	a.newTab()
	a.selectDocumentTab(active, false)
	tr := ui.NewTester(a.View, 1120, 680)
	tr.Frame()
	bounds := func(label string) ui.Rect {
		t.Helper()
		r, ok := tr.Find(label)
		if !ok || r.W == 0 || r.H == 0 {
			t.Fatalf("missing visible %q", label)
		}
		return r
	}
	order := func(paths ...string) {
		t.Helper()
		var got []string
		for _, p := range a.documentTab.panes() {
			got = append(got, p.path)
		}
		if !slices.Equal(got, paths) {
			t.Fatalf("split paths = %v, want %v", got, paths)
		}
	}
	edge := bounds("右侧分栏落点")
	dragWorkspaceDocumentEntry(t, tr, "乙.md", edge.X+edge.W/2, edge.Y+90)
	order(first, second)
	divider := bounds("调整 甲.md 与 乙.md 的分栏宽度")
	x, y := divider.X+divider.W/2, divider.Y+90
	tr.Press(x, y)
	tr.Move(x+56, y)
	tr.Release(x+56, y)
	if a.split.panes[0].splitWeight <= a.split.panes[1].splitWeight {
		t.Fatal("dragging the divider did not redistribute the width")
	}
	left, right := bounds("聚焦分栏 甲.md"), bounds("聚焦分栏 乙.md")
	tr.Press(right.X+right.W/2, right.Y+right.H/2)
	tr.Move(left.X+8, left.Y+left.H/2)
	tr.Release(left.X+8, left.Y+left.H/2)
	order(second, first)
	divider = bounds("调整 乙.md 与 甲.md 的分栏宽度")
	if !tr.DropFiles(divider.X+divider.W/2, divider.Y+90, external) {
		t.Fatal("the divider did not accept the system file drop")
	}
	order(second, external, first)
	if a.path != external || a.treeUndo.available() {
		t.Fatal("the split drop did not activate the external document or touched the tree")
	}
	current := bounds("聚焦分栏 丙.md")
	title := bounds("新建标签页")
	tr.Press(current.X+current.W/2, current.Y+current.H/2)
	tr.Move(title.X-30, title.Y+title.H/2)
	tr.Release(title.X-30, title.Y+title.H/2)
	if len(a.tabs) != 3 || a.path != external || a.split != nil || !tr.HasText("乙.md | 甲.md") {
		t.Fatal("dragging the pane to the title bar did not detach it into its own tab")
	}
	if got := a.markdownSource(); !strings.Contains(got, "Third") {
		t.Fatal("detaching the pane lost its document")
	}
}
