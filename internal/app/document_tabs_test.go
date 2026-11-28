package app

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/editor"
)

func tabTestFile(t *testing.T, dir, name, text string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestFileTreeOpenInNewTabKeepsIndependentEditors(t *testing.T) {
	root := isolateConfig(t)
	first := tabTestFile(t, root, "甲.md", "正文甲\n")
	second := tabTestFile(t, root, "乙.md", "正文乙\n")
	a := New(first)
	visual := a.documentTab
	block := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: block.ID, Offset: len(block.Text.Text)}))
	tr := ui.NewTester(a.View, 960, 620)
	tr.Frame()
	tr.Type(" 草稿")
	if err := tr.RightClick(filepath.Base(second)); err != nil {
		t.Fatal(err)
	}
	if err := tr.ChooseMenuItem("打开方式", "在新标签页中打开"); err != nil {
		t.Fatal(err)
	}
	if len(a.tabs) != 2 || a.path != second || a.root != root || !visual.dirty || !tr.HasText("打开的文档") {
		t.Fatalf("tree opening switched to %q, %d tabs", a.path, len(a.tabs))
	}
	if active := a.treeView.selectedPath(); active != "" && active != second {
		t.Fatalf("tree selection = %q, want %q", active, second)
	}
	a.enterSourceAt(a.markdownSource(), len("正文乙"))
	tr.Frame()
	tr.Type(" 源码")
	source := a.source
	selection := source.edit.Selections()[0]
	tr.Key(ui.Ctrl|ui.Shift, ui.KeyTab)
	if a.documentTab != visual || a.source != nil || !tr.Focused("正文") {
		t.Fatal("previous-tab shortcut did not restore the visual document and its caret")
	}
	tr.Type(" 续")
	if got := a.markdownSource(); !strings.Contains(got, "正文甲 草稿 续") {
		t.Fatalf("the caret was not restored: %q", got)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if got := a.markdownSource(); !strings.Contains(got, "正文甲 草稿") || strings.Contains(got, "续") {
		t.Fatalf("undo was not confined to the last edit in this tab: %q", got)
	}
	tr.Key(ui.Ctrl, ui.KeyTab)
	if a.source != source || !strings.Contains(a.markdownSource(), "正文乙 源码") || a.source.edit.Selections()[0] != selection {
		t.Fatal("next-tab shortcut lost source mode, its edit or its selection")
	}
	a.openWorkspaceFile(first, workspaceOpenTab)
	if a.documentTab != visual || len(a.tabs) != 2 {
		t.Fatal("opening the same tree entry created a duplicate tab")
	}
	for path, want := range map[string]string{first: "正文甲\n", second: "正文乙\n"} {
		if raw, err := os.ReadFile(path); err != nil || string(raw) != want {
			t.Fatalf("tab switching wrote %q: %q, %v", path, raw, err)
		}
	}
}

func TestFileTreeMoveAndExternalRenameFollowBackgroundTab(t *testing.T) {
	root := isolateConfig(t)
	original := tabTestFile(t, root, "原稿.md", utf8BOM+"正文\r\n")
	other := tabTestFile(t, root, "当前.md", "切换后编辑\n")
	folder := filepath.Join(root, "归档")
	if err := os.Mkdir(folder, 0o700); err != nil {
		t.Fatal(err)
	}
	a := New(original)
	d := desktopWindows{active: a, order: []*App{a}}
	a.windows = &d
	background := a.documentTab
	a.enterSourceAt(a.markdownSource(), len("正文"))
	a.source.edit.Insert(" 已修订")
	a.openDocumentInNewTab(other, 1)
	renamed := filepath.Join(root, "改名.md")
	if err := os.Rename(original, renamed); err != nil {
		t.Fatal(err)
	}
	a.refreshFiles()
	if background.path != renamed || a.path != other {
		t.Fatalf("external rename switched or lost the background tab: %q / %q", a.path, background.path)
	}
	if err := a.moveTreeInto(a.tree.at(renamed), a.tree.at(folder)); err != nil {
		t.Fatal(err)
	}
	moved := filepath.Join(folder, filepath.Base(renamed))
	if background.path != moved || a.path != other {
		t.Fatalf("tree move did not update only the background tab: %q / %q", a.path, background.path)
	}
	if err := a.undoTreeOperation(); err != nil {
		t.Fatal(err)
	}
	if background.path != renamed || a.path != other {
		t.Fatalf("move undo changed the wrong tab: %q / %q", a.path, background.path)
	}
	if d.activateFile(renamed) != a || a.documentTab != background || a.source == nil || !a.dirty || len(a.tabs) != 2 {
		t.Fatal("an OS open request did not reactivate the existing edited tab")
	}
	a.Save()
	if raw, err := os.ReadFile(renamed); err != nil || string(raw) != utf8BOM+"正文 已修订\r\n" {
		t.Fatalf("save after tab move = %q, %v", raw, err)
	}
}

func TestExplicitTabOpenCloseReopenAndBackgroundSave(t *testing.T) {
	root := isolateConfig(t)
	first := tabTestFile(t, root, "甲.md", "First\n")
	second := tabTestFile(t, root, "乙.md", "Second\n")
	a := New(first)
	a.autoSaveOn = true
	a.enterSourceAt(a.markdownSource(), len("First"))
	tr := ui.NewTester(a.View, 960, 620)
	tr.Frame()
	tr.Type(" draft")
	a.openWorkspaceFile(second, workspaceOpenTab)
	tr.Frame()
	if len(a.tabs) != 2 || a.path != second {
		t.Fatal("explicit tab open did not create a tab")
	}
	tr.Advance(2 * time.Second)
	if raw, err := os.ReadFile(first); err != nil || string(raw) != "First draft\n" {
		t.Fatalf("background auto-save = %q, %v", raw, err)
	}
	if saved := loadSettings().Last; a.path != second || saved != second {
		t.Fatalf("background auto-save switched the active or remembered file: %q / %q", a.path, saved)
	}
	tr.Key(ui.Cmd, ui.KeyW)
	if len(a.tabs) != 1 || a.path != first {
		t.Fatal("close-tab shortcut did not reveal the remaining tab")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyT)
	if len(a.tabs) != 2 || a.path != second || a.tabIndex() != 1 {
		t.Fatal("reopen shortcut did not restore the closed tab on the right")
	}
	a.saveMarkdown(first)
	if a.path != second || !strings.Contains(a.status, "另一个标签页") {
		t.Fatal("Save As could replace a file open in the other tab")
	}
}

func TestFileTreeAndNativeDropsOpenAndReorderTabs(t *testing.T) {
	root := isolateConfig(t)
	first := tabTestFile(t, root, "甲.md", "First\n")
	second := tabTestFile(t, root, "乙.md", "Second\n")
	external := t.TempDir()
	third := tabTestFile(t, external, "丙.md", "Third\n")
	spare := tabTestFile(t, external, "丁.md", "Spare\n")
	a := New(first)
	a.openDocumentInNewTab(spare, 1)
	tr := ui.NewTester(a.View, 960, 620)
	tr.Frame()
	order := func(want ...string) {
		t.Helper()
		var got []string
		for _, tab := range a.tabs {
			got = append(got, tab.path)
		}
		if !slices.Equal(got, want) {
			t.Fatalf("tabs = %v, want %v", got, want)
		}
	}
	bounds := func(label string) ui.Rect {
		t.Helper()
		r, ok := tr.Find(label)
		if !ok || r.W == 0 || r.H == 0 {
			t.Fatalf("missing visible %q", label)
		}
		return r
	}
	from := bounds("乙.md")
	firstTab := bounds("关闭 甲.md")
	x, y := firstTab.X-90, firstTab.Y+firstTab.H/2
	tr.Press(from.X+from.W/2, from.Y+from.H/2)
	tr.Move(x, y)
	tr.Release(x, y)
	order(second, first, spare)
	if a.path != second || !workspaceTestFocused(a, tr, "乙.md") && a.activeWorkspacePath() != second {
		t.Fatal("tree drop did not activate the document and reveal it in the tree")
	}
	firstTab = bounds("关闭 甲.md")
	if !tr.DropFiles(firstTab.X-4, firstTab.Y+firstTab.H/2, third) || a.path != third {
		t.Fatal("system file drop was not opened as a tab")
	}
	order(second, first, third, spare)
	secondTab, thirdTab := bounds("关闭 乙.md"), bounds("关闭 丙.md")
	fromX, tabY := thirdTab.X-75, thirdTab.Y+thirdTab.H/2
	toX := secondTab.X - 100
	tr.Press(fromX, tabY)
	tr.Move(toX, tabY)
	tr.Key(0, ui.KeyEscape)
	tr.Release(toX, tabY)
	order(second, first, third, spare)
	tr.Press(fromX, tabY)
	tr.Move(toX, tabY)
	tr.Release(toX, tabY)
	order(third, second, first, spare)
	if a.path != third || a.treeUndo.available() {
		t.Fatal("reordering changed the current document or the workspace on disk")
	}
	firstTab = bounds("关闭 甲.md")
	if !tr.DropFiles(firstTab.X-4, firstTab.Y+firstTab.H/2, third) {
		t.Fatal("drop on another tab was rejected")
	}
	order(third, second, first, spare)
	if !tr.DropFiles(firstTab.X-4, firstTab.Y+firstTab.H/2, external) || a.root != external || a.path != third {
		t.Fatal("dropping a folder did not open its tree while retaining the document")
	}
}
