package app

import (
	"bytes"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func workspaceTestClick(t *testing.T, tr *ui.Tester, label string) {
	t.Helper()
	if row, ok := workspaceTreeRow(tr, label); ok {
		tr.ClickAt(row.X+row.W/2, row.Y+row.H/2)
		return
	}
	if err := tr.Click(label); err != nil {
		t.Fatal(err)
	}
}

func workspaceTestMenu(t *testing.T, tr *ui.Tester, row, action string) {
	t.Helper()
	if bounds, ok := workspaceTreeRow(tr, row); ok {
		tr.RightClickAt(bounds.X+bounds.W/2, bounds.Y+bounds.H/2)
	} else if err := tr.RightClick(row); err != nil {
		t.Fatal(err)
	}
	if err := tr.ChooseMenuItem(action); err != nil {
		t.Fatalf("%s > %s: %v", row, action, err)
	}
}

func workspaceTreeRow(tr *ui.Tester, name string) (ui.Rect, bool) {
	row, ok := tr.AccessByRole(ui.RoleTreeItem, name)
	return row.Bounds, ok && !row.Offscreen && row.Bounds.W > 0 && row.Bounds.H > 0
}

func workspaceHasTreeRow(tr *ui.Tester, name string) bool {
	_, found := workspaceTreeRow(tr, name)
	return found
}

func workspaceTestFocused(a *App, tr *ui.Tester, label string) bool {
	return tr.Focused("文件目录树") && filepath.Base(a.treeView.selectedPath()) == label
}

func TestFileTreeWidthFollowsMouseAndKeysAndSurvivesRestart(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "项目手册")
	chapter := filepath.Join(root, "团队和项目流程")
	if err := os.MkdirAll(chapter, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(chapter, "研发工作区的新成员交接和交付前自查.md"), []byte("# 新成员交接\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 1120, 680)
	workspaceTestClick(t, tr, "团队和项目流程")
	gripAt := func(tr *ui.Tester, width float32) ui.Rect {
		t.Helper()
		grip, ok := tr.Find("调整侧栏宽度")
		if !ok || grip.X != width || grip.W != sidebarGripWidth {
			t.Fatalf("resize handle at %+v, want X=%v and W=%v", grip, width, sidebarGripWidth)
		}
		return grip
	}
	grip := gripAt(tr, defaultSidebarWidth)
	x, y := grip.X+grip.W/2, grip.Y+grip.H/2
	tr.Press(x, y)
	tr.Move(x+250, y)
	gripAt(tr, defaultSidebarWidth+250)
	if got := loadSettings().SidebarWidth; got != defaultSidebarWidth {
		t.Fatalf("a drag persisted before release: %v", got)
	}
	tr.Release(x+250, y)
	tr.Key(0, ui.KeyRight)
	want := defaultSidebarWidth + 250 + sidebarResizeStep
	grip = gripAt(tr, want)
	if got := loadSettings().SidebarWidth; got != want {
		t.Fatalf("drag or keyboard resize was not persisted: %v, want %v", got, want)
	}
	x = grip.X + grip.W/2
	tr.Press(x, y)
	tr.Move(x-60, y)
	gripAt(tr, want-60)
	tr.CancelPress()
	gripAt(tr, want)
	tr.SetSize(760, 680)
	narrow := gripAt(tr, 760-minEditorWidth-sidebarGripWidth)
	tr.Key(0, ui.KeyRight)
	narrowX := narrow.X + narrow.W/2
	tr.Press(narrowX, y)
	tr.Move(narrowX+30, y)
	tr.Release(narrowX+30, y)
	if got := loadSettings().SidebarWidth; got != want {
		t.Fatalf("a resize at the visible limit changed the remembered width: %v", got)
	}
	tr.SetSize(1120, 680)
	gripAt(tr, want)
	restarted := ui.NewTester(New(root).View, 1120, 680)
	gripAt(restarted, want)
	restarted.ClickAt(x, y)
	restarted.ClickAt(x, y)
	gripAt(restarted, defaultSidebarWidth)
	if got := loadSettings().SidebarWidth; got != defaultSidebarWidth {
		t.Fatalf("double-click reset was not persisted: %v", got)
	}
	saveWorkspaceSnapshot(t, tr, "23-workspace-resized")
}

func TestFileTreeSidebarToggleKeepsTypingInActiveDocument(t *testing.T) {
	isolateConfig(t)
	file := filepath.Join(t.TempDir(), "正文.md")
	if err := os.WriteFile(file, []byte("正文\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(file)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	box := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	tr.ClickAt(box.X+12, box.Y+box.H/2)
	tr.Key(0, ui.KeyEnd)
	tr.Type("一")
	tr.Key(ui.Cmd|ui.Shift, ui.KeyL)
	if workspaceHasTreeRow(tr, "正文.md") || !a.hideSide {
		t.Fatal("the sidebar shortcut did not hide the file tree")
	}
	tr.Type("二")
	tr.Key(ui.Cmd|ui.Shift, ui.KeyL)
	if !workspaceHasTreeRow(tr, "正文.md") || a.hideSide {
		t.Fatal("the sidebar shortcut did not restore the file tree")
	}
	tr.Type("三")
	if got := a.markdownSource(); got != "正文一二三\n" {
		t.Fatalf("toggling the sidebar detached the editor focus or caret: %q", got)
	}
}

func TestFileTreeIsTheSidebarAndOpensNestedDocumentsWithKeys(t *testing.T) {
	dir := isolateConfig(t)
	root := filepath.Join(dir, "笔记库")
	chapter := filepath.Join(root, "章2")
	for _, path := range []string{chapter, filepath.Join(root, "章10"), filepath.Join(root, ".git")} {
		if err := os.MkdirAll(path, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	file := filepath.Join(chapter, "记录.md")
	for p, body := range map[string]string{
		file: "# 今日记录\n", filepath.Join(root, "2-导读.md"): "体验", filepath.Join(root, "10-总结.md"): "内容",
		filepath.Join(root, ".git", "私有.md"): "metadata", filepath.Join(root, "封面.png"): "unsupported",
	} {
		if err := os.WriteFile(p, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New("")
	a.root = root
	a.refreshFiles()
	a.hideSide = true
	tr := ui.NewTester(a.View, 960, 680)
	mods := ui.Cmd | ui.Shift
	if runtime.GOOS == "darwin" {
		mods = ui.Cmd | ui.Ctrl
	}
	tr.Key(mods, ui.Key3)
	if a.hideSide || !workspaceTestFocused(a, tr, "笔记库") || !tr.HasText("章10") || tr.HasText("记录.md") {
		t.Fatal("the file-tree shortcut did not show the focused root and collapsed branches")
	}
	for _, forbidden := range []string{"近期浏览", "模板参考", "标题大纲", "私有.md", "封面.png"} {
		if tr.HasText(forbidden) {
			t.Fatalf("the sidebar unexpectedly shows %q", forbidden)
		}
	}
	visible := tr.Texts()
	if !(slices.Index(visible, "章2") < slices.Index(visible, "章10") && slices.Index(visible, "2-导读.md") < slices.Index(visible, "10-总结.md")) {
		t.Fatalf("unexpected directory or file sort order: %q", visible[:min(12, len(visible))])
	}
	tr.Key(0, ui.KeyDown)
	tr.Key(0, ui.KeyRight)
	if !workspaceTestFocused(a, tr, "章2") || !tr.HasText("记录.md") {
		t.Fatal("Right did not expand the focused chapter")
	}
	tr.Key(0, ui.KeyRight)
	if !workspaceTestFocused(a, tr, "记录.md") {
		t.Fatal("Right did not move to the expanded directory's first file")
	}
	tr.Key(0, ui.KeyEnter)
	if !samePath(a.path, file) || a.core.Doc.Blocks[0].Text.Text != "今日记录" {
		t.Fatalf("Enter did not open %q", file)
	}
	tr.Key(0, ui.KeyLeft)
	if !workspaceTestFocused(a, tr, "章2") {
		t.Fatal("Left did not focus the parent directory")
	}
	tr.Key(0, ui.KeyLeft)
	if workspaceHasTreeRow(tr, "记录.md") {
		t.Fatal("Left did not collapse the directory")
	}
	tr.Key(mods, ui.Key3)
	if !workspaceTestFocused(a, tr, "记录.md") || !a.treeView.Open.Has(chapter) {
		t.Fatal("the file-tree shortcut did not reveal the active file")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyL)
	if !a.hideSide || workspaceHasTreeRow(tr, "记录.md") {
		t.Fatal("the sidebar shortcut did not hide the tree")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyL)
	if a.hideSide || !workspaceHasTreeRow(tr, "记录.md") {
		t.Fatal("the sidebar shortcut did not restore the tree at the open file")
	}
}

func TestFileTreeVirtualizesRowsAndRevealsOffscreenSelection(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	const files = 1600
	for i := range files {
		path := filepath.Join(root, fmt.Sprintf("Note %04d.md", i))
		if err := os.WriteFile(path, nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	last := fmt.Sprintf("Note %04d.md", files-1)
	if a.treeView.Rows() != files+1 || tr.HasText(last) || !tr.HasText("Note 0000.md") {
		t.Fatal("the tree did not load all files while building only the rows in view")
	}
	tr.Key(0, ui.KeyEnd)
	if !workspaceTestFocused(a, tr, last) || !tr.HasText(last) {
		t.Fatal("End did not focus and reveal the last file outside the original viewport")
	}
	tr.Key(0, ui.KeyEnter)
	if want := filepath.Join(root, last); a.path != want {
		t.Fatalf("Enter opened %q, want %q", a.path, want)
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("排序", "降序"); err != nil {
		t.Fatal(err)
	}
	if a.treeView.selectedPath() != a.path || !tr.HasText(last) {
		t.Fatal("reversing the order lost a selection that moved more than one viewport")
	}
	mods := ui.Cmd | ui.Shift
	if runtime.GOOS == "darwin" {
		mods = ui.Cmd | ui.Ctrl
	}
	tr.Key(mods, ui.Key3)
	tr.Key(0, ui.KeyEnd)
	if !workspaceTestFocused(a, tr, "Note 0000.md") || workspaceHasTreeRow(tr, last) {
		t.Fatal("End did not navigate to the other end after a sort")
	}
	tr.Key(mods, ui.Key3)
	if !workspaceTestFocused(a, tr, last) || !tr.HasText(last) || a.dirty {
		t.Fatal("the file-tree shortcut did not return to the active file from outside the viewport")
	}
	newPath := filepath.Join(root, "Note 9999.md")
	if err := os.WriteFile(newPath, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	workspaceTestMenu(t, tr, last, "复制路径")
	a.refreshFiles()
	tr.Frame()
	if a.treeView.Rows() != files+2 || !workspaceTestFocused(a, tr, last) {
		t.Fatal("refresh dropped the selected file when another file arrived above it")
	}
	saveWorkspaceSnapshot(t, tr, "19-workspace-virtualized")
}

func TestFileTreeKeepsWorkspaceMenuAtTheTopWhileScrolling(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "项目笔记")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	for i := range 80 {
		path := filepath.Join(root, fmt.Sprintf("Note %03d.md", i))
		if err := os.WriteFile(path, nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	initial, ok := tr.Find("文件夹操作")
	if !ok {
		t.Fatal("the workspace menu was missing from the root row")
	}
	tr.Key(0, ui.KeyEnd)
	if !workspaceTestFocused(a, tr, "Note 079.md") || tr.HasText("Note 000.md") {
		t.Fatal("End did not scroll the directory tree away from its first entries")
	}
	pinned, ok := tr.Find("文件夹操作")
	if !ok || pinned.Y < 0 || pinned.Y > initial.Y+4 {
		t.Fatalf("the workspace menu scrolled away: %+v (found %v)", pinned, ok)
	}
	lastRow, found := tr.Find("Note 079.md")
	footer, hasFooter := tr.Find("新建文件或文件夹")
	if !found || !hasFooter || lastRow.Y+lastRow.H > footer.Y-6 {
		t.Fatal("pinning the menu hid the last selected row below the viewport")
	}
	tr.Key(ui.Shift, ui.KeyTab)
	if !tr.Focused("文件夹操作") {
		t.Fatal("the persistent menu is out of the tree's keyboard tab order")
	}
	tr.Key(0, ui.KeyEnter)
	if !slices.Contains(tr.Menu(), "在其他应用中打开") {
		t.Fatal("the folder menu does not offer its external app actions")
	}
	if err := tr.ChooseMenuItem("复制路径"); err != nil {
		t.Fatal(err)
	}
	if tr.Clipboard() != root || !tr.HasText("Note 079.md") {
		t.Fatal("the pinned menu acted on the scrolled row or jumped away from it")
	}
	saveWorkspaceSnapshot(t, tr, "20-workspace-pinned-menu")
}

func TestFileTreeRefreshesAnOpenDirectoryAndKeepsItExpanded(t *testing.T) {
	dir := isolateConfig(t)
	root := filepath.Join(dir, "工作区")
	chapter := filepath.Join(root, "子目录")
	if err := os.MkdirAll(chapter, 0o700); err != nil {
		t.Fatal(err)
	}
	a := New("")
	a.root = root
	a.refreshFiles()
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	if err := tr.Click("子目录"); err != nil {
		t.Fatal(err)
	}
	p := filepath.Join(chapter, "后来加入.md")
	if err := os.WriteFile(p, []byte("添加的文档"), 0o600); err != nil {
		t.Fatal(err)
	}
	// Make the modification observable even on a coarse timestamp file system.
	now := time.Now().Add(2 * time.Second)
	if err := os.Chtimes(chapter, now, now); err != nil {
		t.Fatal(err)
	}
	tr.Advance(1200 * time.Millisecond)
	if !tr.HasText("后来加入.md") || !a.treeView.Open.Has(chapter) || len(a.files) != 1 {
		t.Fatal("file tree did not update the open branch")
	}
}

func TestFileTreeCreatesRenamesMovesAndCopiesThroughNativeControls(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "工作区")
	target := filepath.Join(root, "归档")
	if err := os.MkdirAll(target, 0o700); err != nil {
		t.Fatal(err)
	}
	oldPath := filepath.Join(root, "原文.md")
	original := []byte(utf8BOM + "# 原文\r\n\r\n段落\r\n")
	if err := os.WriteFile(oldPath, original, 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(oldPath)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	rootRow, rootOK := tr.Find("工作区")
	actions, actionsOK := tr.Find("文件夹操作")
	if !rootOK || !actionsOK || actions.Y < rootRow.Y || actions.Y+actions.H > rootRow.Y+rootRow.H {
		t.Fatal("folder actions are out of reach of the root row")
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if !a.treeView.Open.Has(root) {
		t.Fatal("opening the root actions collapsed the tree")
	}
	if err := tr.ChooseMenuItem("新建文件夹…"); err != nil {
		t.Fatal(err)
	}
	tr.Type("草稿")
	tr.Key(0, ui.KeyEnter)
	if p := filepath.Join(root, "草稿"); !workspaceTestFocused(a, tr, "草稿") || !a.treeView.Open.Has(root) {
		t.Fatalf("did not reveal and focus %s", p)
	}
	workspaceTestMenu(t, tr, "草稿", "新建文件…")
	tr.Type("一页")
	tr.Key(0, ui.KeyEnter)
	newFile := filepath.Join(root, "草稿", "一页.md")
	if a.path != newFile || !tr.HasText("一页.md") {
		t.Fatalf("new file was not opened: %q (%s)", a.path, a.status)
	}
	workspaceTestClick(t, tr, "原文.md")
	a.toggleSource()
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyEnd)
	tr.Type("未保存")
	if !a.dirty || !bytes.Contains([]byte(a.markdownSource()), []byte("未保存")) {
		t.Fatal("source edit was not retained")
	}
	liveSource := a.source
	mods := ui.Cmd | ui.Shift
	if runtime.GOOS == "darwin" {
		mods = ui.Cmd | ui.Ctrl
	}
	tr.Key(mods, ui.Key3)
	tr.Key(0, ui.KeyF2)
	tr.Type("新名")
	tr.Key(0, ui.KeyEnter)
	renamed := filepath.Join(root, "新名.md")
	if a.path != renamed || a.file.path != renamed || a.source != liveSource || !a.dirty {
		t.Fatalf("rename detached the modified editor: %q %q (%s)", a.path, a.file.path, a.treeEdit.err)
	}
	if _, err := os.Stat(oldPath); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("original name remains: %v", err)
	}
	sourceBox, srcOK := workspaceTreeRow(tr, "新名.md")
	destBox, dstOK := tr.Find("归档")
	if !srcOK || !dstOK {
		t.Fatal("move source or destination is missing")
	}
	x, y := sourceBox.X+sourceBox.W/2, sourceBox.Y+sourceBox.H/2
	dx, dy := destBox.X+destBox.W/2, destBox.Y+destBox.H/2
	tr.Press(x, y)
	tr.Move(dx, dy)
	tr.Release(dx, dy)
	moved := filepath.Join(target, "新名.md")
	if a.treeEdit.open || a.path != moved || a.file.path != moved || a.source != liveSource || !a.dirty || !tr.HasText("新名.md") {
		t.Fatalf("move detached the editor or hid the target: %q (%s)", a.path, a.treeEdit.err)
	}
	a.Save()
	saved, err := os.ReadFile(moved)
	if err != nil || !bytes.HasPrefix(saved, []byte(utf8BOM)) ||
		!bytes.Contains(saved, []byte("\r\n")) || !bytes.Contains(saved, []byte("未保存")) || a.dirty {
		t.Fatalf("save after moving: %q, %v (%s)", saved, err, a.status)
	}
	workspaceTestMenu(t, tr, "新名.md", "撤销上次文件操作")
	if a.path != renamed || a.source != liveSource || a.dirty {
		t.Fatalf("undo detached the editor: %q (%s)", a.path, a.status)
	}
	workspaceTestMenu(t, tr, "新名.md", "制作副本")
	duplicate := filepath.Join(root, "新名 副本.md")
	for _, path := range []string{renamed, duplicate} {
		if data, err := os.ReadFile(path); err != nil || !bytes.Equal(data, saved) {
			t.Fatalf("undo or copy did not preserve the saved bytes: %q, %v", data, err)
		}
	}
}

func TestFileTreeCreatesFromTemplateWithKeyboardAndKeepsExistingFiles(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "工作区")
	child := filepath.Join(root, "项目")
	if err := os.MkdirAll(child, 0o700); err != nil {
		t.Fatal(err)
	}
	current := filepath.Join(root, "日记.md")
	occupied := filepath.Join(child, "已存在.md")
	for _, path := range []string{current, occupied} {
		if err := os.WriteFile(path, []byte("# 已有内容\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(current)
	// The template arrives after MarkdownX has opened the workspace.
	body := utf8BOM + "---\r\ntags: [计划]\r\n---\r\n\r\n# 定制模板\r\n\r\n原文  \r\n"
	if _, err := saveTemplate("周计划", body); err != nil {
		t.Fatal(err)
	}
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestMenu(t, tr, "项目", "从模板新建…")
	if !tr.Focused("选择模板") || !tr.HasText(child) {
		t.Fatalf("the template dialog did not show and focus the selection for %s", child)
	}
	saveWorkspaceSnapshot(t, tr, "17-workspace-template")
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	if a.treeEdit.name != "周计划" {
		t.Fatalf("template choice did not set the filename: %q", a.treeEdit.name)
	}
	tr.Key(0, ui.KeyTab)
	if !tr.Focused("文件名") {
		t.Fatal("the template picker did not move focus to the filename")
	}
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("已存在")
	tr.Key(0, ui.KeyEnter)
	if !a.treeEdit.open || a.treeEdit.err == "" || a.path != current {
		t.Fatal("a collision did not retain the editor and the template dialog")
	}
	if data, err := os.ReadFile(occupied); err != nil || string(data) != "# 已有内容\n" {
		t.Fatalf("the occupied name was changed: %q, %v", data, err)
	}
	tr.Key(0, ui.KeyEscape)
	if a.treeEdit.open || !workspaceTestFocused(a, tr, "项目") || a.path != current {
		t.Fatal("Escape did not close the dialog and focus its directory again")
	}
	tr.Key(0, ui.KeyRight)
	tr.Key(0, ui.KeyRight)
	if !workspaceTestFocused(a, tr, "已存在.md") {
		t.Fatal("the existing file was not reachable from the restored directory focus")
	}
	tr.Key(ui.Shift, ui.KeyF10)
	if err := tr.ChooseMenuItem("从模板新建…"); err != nil {
		t.Fatal(err)
	}
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyTab)
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("定稿.md")
	tr.Key(0, ui.KeyEnter)
	created := filepath.Join(child, "定稿.md")
	if a.treeEdit.open || a.path != created || a.dirty || !workspaceTestFocused(a, tr, "定稿.md") {
		t.Fatalf("the new document did not open and reveal in the same directory: %q (%q)", a.path, a.treeEdit.err)
	}
	if data, err := os.ReadFile(created); err != nil || string(data) != body {
		t.Fatalf("template BOM, line endings, or content changed: %q, %v", data, err)
	}
	if entries, err := os.ReadDir(child); err != nil || len(entries) != 2 {
		t.Fatalf("creation or retry left unexpected entries: %v, %v", entries, err)
	}
}

func TestFileTreeRefusesToOverwriteOrUndoOverNewContent(t *testing.T) {
	root := isolateConfig(t)
	src := filepath.Join(root, "初稿.md")
	existing := filepath.Join(root, "报告.md")
	if err := os.WriteFile(src, []byte("原文"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(existing, []byte("已存在"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(src)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestMenu(t, tr, "初稿.md", "重命名…")
	tr.Type("报告")
	tr.Key(0, ui.KeyEnter)
	if !a.treeEdit.open || a.treeEdit.err == "" || a.path != src {
		t.Fatal("rename over a different document did not stay in the dialog")
	}
	if data, err := os.ReadFile(existing); err != nil || string(data) != "已存在" {
		t.Fatalf("an existing document was replaced: %q, %v", data, err)
	}
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("定稿")
	tr.Key(0, ui.KeyEnter)
	dst := filepath.Join(root, "定稿.md")
	if a.path != dst {
		t.Fatalf("rename could not be retried: %q, %q", a.path, a.treeEdit.err)
	}
	if err := os.WriteFile(src, []byte("外部已占用"), 0o600); err != nil {
		t.Fatal(err)
	}
	workspaceTestMenu(t, tr, "定稿.md", "撤销上次文件操作")
	if a.path != dst || len(a.treeUndo.moves) != 1 || a.treeUndo.moves[0].from != dst {
		t.Fatal("undo replaced an occupied original name or could not be retried")
	}
	if data, err := os.ReadFile(src); err != nil || string(data) != "外部已占用" {
		t.Fatalf("undo clobbered an external document: %q, %v", data, err)
	}
	if err := os.Remove(src); err != nil {
		t.Fatal(err)
	}
	workspaceTestMenu(t, tr, "定稿.md", "撤销上次文件操作")
	if a.path != src {
		t.Fatalf("undo did not recover after the collision was cleared: %s", a.status)
	}
	tr.Key(treeTrashKey())
	if !a.treeEdit.open || a.treeEdit.action != workspaceTrash {
		t.Fatal("tree trash key did not show confirmation")
	}
	tr.Key(0, ui.KeyEscape)
	if _, err := os.Stat(src); err != nil || a.treeEdit.open {
		t.Fatalf("cancel did not preserve the document: %v", err)
	}
}

func TestFileTreeSortAndExplorerDrop(t *testing.T) {
	dir := isolateConfig(t)
	root := filepath.Join(dir, "工作区")
	external := filepath.Join(dir, "外部.md")
	for _, p := range []string{filepath.Join(root, "Z"), filepath.Join(root, "B")} {
		if err := os.MkdirAll(p, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	for _, p := range []string{filepath.Join(root, "2.md"), filepath.Join(root, "10.md"), external} {
		if err := os.WriteFile(p, []byte(filepath.Base(p)), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New("")
	a.root = root
	a.refreshFiles()
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	for _, choice := range []string{"名称", "降序", "文件夹排在前面"} {
		workspaceTestClick(t, tr, "文件夹操作")
		if err := tr.ChooseMenuItem("排序", choice); err != nil {
			t.Fatal(err)
		}
	}
	if have := tr.Texts(); !(slices.Index(have, "Z") < slices.Index(have, "B") &&
		slices.Index(have, "B") < slices.Index(have, "2.md") && slices.Index(have, "2.md") < slices.Index(have, "10.md")) {
		t.Fatalf("wrong mixed sort order: %q", have)
	}
	if persisted := New("").treeOrder; persisted != (workspaceOrder{By: workspaceAlphabetical, Descending: true, Mix: true}) {
		t.Fatalf("order did not persist: %+v", persisted)
	}
	dest, _ := tr.Find("Z")
	if !tr.DropFiles(dest.X+dest.W/2, dest.Y+dest.H/2, external) {
		t.Fatal("tree did not accept a file dropped from another app")
	}
	copied := filepath.Join(root, "Z", "外部.md")
	for _, p := range []string{copied, external} {
		if data, err := os.ReadFile(p); err != nil || string(data) != "外部.md" {
			t.Fatalf("external drop lost %q: %q, %v", p, data, err)
		}
	}
	if !workspaceTestFocused(a, tr, "外部.md") {
		t.Fatal("external drop did not reveal the copied file")
	}
}
