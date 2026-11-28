package app

import (
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeQuickOpenLocatesNestedResultWithoutOpeningIt(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	parent := filepath.Join(root, "项目", "资料")
	if err := os.MkdirAll(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	current := filepath.Join(root, "当前.md")
	target := filepath.Join(parent, "定位目标.md")
	outside := filepath.Join(t.TempDir(), "外部笔记.md")
	for path, body := range map[string]string{current: "正在编辑\n", target: "目标内容\n", outside: "外部内容\n"} {
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	saveSettings(settings{Recent: []string{outside}})
	a := New(current)
	a.hideSide = true
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	box := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	tr.ClickAt(box.X+12, box.Y+box.H/2)
	tr.Key(0, ui.KeyEnd)
	tr.Type("未保存")
	quickOpen := func() {
		if runtime.GOOS == "darwin" {
			tr.Key(ui.Cmd|ui.Shift, ui.KeyO)
		} else {
			tr.Key(ui.Cmd, ui.KeyP)
		}
		if !a.quickOpen || !tr.Focused("快速打开") {
			t.Fatal("the quick-open shortcut did not open and focus the search")
		}
	}
	quickOpen()
	tr.Type("定位目标")
	tr.Key(ui.Cmd, ui.KeyK)
	if !a.quickActions || !tr.HasText("在目录树中显示") {
		t.Fatal("the quick-open actions shortcut did not open the location menu")
	}
	saveWorkspaceSnapshot(t, tr, "24-workspace-quick-locate")
	tr.Key(0, ui.KeyEscape)
	if !a.quickOpen || a.quickActions || !tr.Focused("快速打开") {
		t.Fatal("Escape did not return from quick-open actions to the query")
	}
	tr.Key(ui.Cmd, ui.KeyK)
	tr.Key(ui.Cmd, ui.KeyK)
	if !a.quickOpen || a.quickActions || !tr.Focused("快速打开") {
		t.Fatal("the action shortcut did not return to the query while the menu was open")
	}
	tr.Key(ui.Cmd, ui.KeyK)
	tr.Key(0, ui.KeyDown)
	tr.Key(0, ui.KeyEnter)
	if a.quickOpen || a.hideSide || !workspaceTestFocused(a, tr, filepath.Base(target)) || !a.treeView.Open.Has(parent) {
		t.Fatal("the location action did not reveal and focus the collapsed file")
	}
	if a.path != current || !a.dirty || a.markdownSource() != "正在编辑未保存\n" {
		t.Fatal("locating the file changed the active document")
	}
	quickOpen()
	tr.Type("外部笔记")
	workspaceTestClick(t, tr, "操作")
	if !a.quickActions || tr.HasText("在目录树中显示") {
		t.Fatal("a file outside the workspace offered to reveal a tree row")
	}
}

func TestFileTreeShowsMarkdownExcerptAfterHoverWithoutOpeningFile(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	current := filepath.Join(root, "Current.md")
	other := filepath.Join(root, "Preview.md")
	for path, body := range map[string]string{
		current: "在编辑的文档\n",
		other:   "# 预览标题\n\n预览正文只在悬停时出现。\n",
	} {
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(current)
	tr := ui.NewTester(a.View, 980, 700)
	tr.Frame()
	box, ok := tr.Find("Preview.md")
	if !ok {
		t.Fatal("preview row did not render")
	}
	tr.Move(box.X+box.W/2, box.Y+box.H/2)
	if tr.HasText("预览标题") {
		t.Fatal("preview showed before the hover delay")
	}
	tr.Advance(730 * time.Millisecond)
	if !tr.HasText("预览标题") || !tr.HasText("预览正文只在悬停时出现。") || a.path != current {
		t.Fatalf("hover did not render the excerpt or replaced the editor: %q", tr.Texts())
	}
	saveWorkspaceSnapshot(t, tr, "15-file-tree-hover")
	tr.Move(680, 500)
	if tr.HasText("预览标题") || a.path != current {
		t.Fatal("preview did not go away on pointer leave")
	}
}

func TestFileTreeShowsRasterThumbnailWhenAllFilesAreVisible(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	path := filepath.Join(root, "Sample.png")
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	bitmap := image.NewRGBA(image.Rect(0, 0, 32, 18))
	bitmap.Set(4, 4, color.RGBA{R: 156, G: 80, B: 190, A: 255})
	if err := png.Encode(f, bitmap); err != nil {
		_ = f.Close()
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
	a := New("")
	a.treeFilter.Mode = workspaceAllFiles
	a.openWorkspaceFolder(root)
	tr := ui.NewTester(a.View, 980, 700)
	tr.Frame()
	row, ok := tr.Find("Sample.png")
	if !ok {
		t.Fatal("image row did not appear")
	}
	tr.Move(row.X+row.W/2, row.Y+row.H/2)
	tr.Advance(730 * time.Millisecond)
	if a.treePreview.picture == nil || a.path != "" {
		t.Fatal("hover did not decode the local image")
	}
	if w, h := ui.ImageSize(a.treePreview.picture); w != 32 || h != 18 {
		t.Fatalf("thumbnail dimensions = %v x %v", w, h)
	}
}

func TestFileTreePinnedFoldersSurviveRenameAndOpenFromSavedMenus(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	original := filepath.Join(root, "Research")
	renamed := filepath.Join(root, "Reference")
	if err := os.Mkdir(original, 0o700); err != nil {
		t.Fatal(err)
	}
	note := filepath.Join(original, "Note.md")
	if err := os.WriteFile(note, []byte("本地资料"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New("")
	a.openWorkspaceFolder(root)
	s := loadSettings()
	s.DefaultWorkspace = original
	saveSettings(s)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestMenu(t, tr, "Research", "固定文件夹")
	workspaceTestMenu(t, tr, "Research", "重命名…")
	tr.Type("Reference")
	tr.Key(0, ui.KeyEnter)
	if s := loadSettings(); !slices.Equal(s.PinnedFolders, []string{renamed}) || s.DefaultWorkspace != renamed {
		t.Fatalf("saved folders did not follow rename: %q, %q", s.PinnedFolders, s.DefaultWorkspace)
	}
	b := New("")
	tr = ui.NewTester(b.View, 960, 680)
	tr.Frame()
	workspaceTestClick(t, tr, "最近和固定的文件夹")
	if err := tr.ChooseMenuItem("固定的文件夹", renamed, "打开文件夹"); err != nil {
		t.Fatal(err)
	}
	if b.root != renamed || !tr.HasText("Note.md") {
		t.Fatalf("saved pin opened the wrong workspace: %q", b.root)
	}
	b.openWorkspaceFolder(root)
	b.openQuickOpen()
	tr.Frame()
	tr.Type("rfrnc")
	if !slices.ContainsFunc(b.quickResults(), func(hit quickHit) bool { return hit.path == renamed }) {
		t.Fatal("the pinned workspace was missing from Quick Open")
	}
	tr.Key(0, ui.KeyEnter)
	if b.root != renamed || b.path != "" {
		t.Fatal("Quick Open did not open the directory as a tree")
	}
	workspaceTestMenu(t, tr, "Reference", "取消固定文件夹")
	if workspaceFolderListed(loadSettings().PinnedFolders, renamed) {
		t.Fatal("unpin did not update folder history")
	}
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("最近的文件夹", renamed, "从最近记录移除"); err != nil {
		t.Fatal(err)
	}
	b.remember()
	if workspaceFolderListed(loadSettings().RecentFolders, renamed) {
		t.Fatal("the active workspace was added back without reopening it")
	}
	if _, err := os.Stat(filepath.Join(renamed, "Note.md")); err != nil {
		t.Fatalf("removing the bookmark touched the real folder: %v", err)
	}
}

func TestFileTreeStartupModesUseSavedFilesAndDefaultWorkspace(t *testing.T) {
	base := isolateConfig(t)
	root := filepath.Join(base, "笔记 & 档案")
	chapter := filepath.Join(root, "章节")
	preferred := filepath.Join(base, "默认文件夹")
	for _, dir := range []string{chapter, preferred} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	note := filepath.Join(chapter, "记录.md")
	if err := os.WriteFile(note, []byte("# 正文\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 760, 420)
	tr.Frame()
	if a.root != root || a.path != "" || !workspaceTestFocused(a, tr, filepath.Base(root)) || !tr.HasText("章节") {
		t.Fatal("the launch argument did not open and focus a workspace tree")
	}
	a.OpenFile(note)
	fresh := New("")
	fresh.restoreWorkspace()
	if fresh.root != "" || fresh.path != "" {
		t.Fatal("a fresh profile did not default to a new document")
	}
	s := loadSettings()
	s.DefaultWorkspace = preferred
	saveSettings(s)
	fresh.restoreWorkspace()
	if fresh.root != preferred || fresh.path != "" {
		t.Fatal("new document did not use its configured default workspace")
	}

	tr.Key(ui.Cmd, ui.KeyComma)
	if !a.prefsOpen || !tr.HasText("默认启动文件夹") {
		t.Fatal("the new document startup settings are missing")
	}
	saveWorkspaceSnapshot(t, tr, "18-workspace-startup")
	workspaceTestClick(t, tr, "清除默认启动文件夹")
	if s := loadSettings(); s.DefaultWorkspace != "" || s.Root != root {
		t.Fatalf("clearing the default changed the saved workspace: %q, %q", s.DefaultWorkspace, s.Root)
	}
	workspaceTestClick(t, tr, "新建文档")
	tr.Key(0, ui.KeyDown)
	tr.Key(0, ui.KeyEnter)
	if s := loadSettings(); s.OnLaunch != workspaceLaunchFolder {
		t.Fatalf("folder-only startup was not saved: %v", s.OnLaunch)
	}
	workspace := New("")
	workspace.restoreWorkspace()
	if workspace.root != root || workspace.path != "" || workspace.tree.at(note) == nil {
		t.Fatal("folder-only startup did not restore the tree with an empty editor")
	}
	workspaceTestClick(t, tr, "恢复上次的文件夹")
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	reopened := New("")
	reopened.restoreWorkspace()
	opened := ui.NewTester(reopened.View, 760, 420)
	opened.Frame()
	if reopened.root != root || reopened.path != note || !opened.HasText("记录.md") {
		t.Fatal("document and folder startup did not reveal the restored document in its branch")
	}
}

func TestFileTreeClosedWorkspaceStaysClosedWhenRestoringLegacyProfile(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	note := filepath.Join(root, "保存的笔记.md")
	if err := os.WriteFile(note, []byte("保存的内容\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	saveSettings(settings{Root: root, Last: note})
	a := New("")
	a.restoreWorkspace()
	if a.root != root || a.path != note {
		t.Fatal("upgrading an existing profile changed its launch behavior")
	}
	tr := ui.NewTester(a.View, 760, 420)
	tr.Frame()
	workspaceTestClick(t, tr, "文件夹操作")
	if err := tr.ChooseMenuItem("关闭文件夹"); err != nil {
		t.Fatal(err)
	}
	if a.root != "" || !tr.Focused("打开文件夹…") || a.path != note {
		t.Fatal("closing the workspace did not preserve the document and focus the sidebar")
	}
	reopened := New("")
	reopened.restoreWorkspace()
	if reopened.tree != nil || reopened.root != "" || reopened.path != note {
		t.Fatal("restoring the document reopened its deliberately closed parent folder")
	}
}

func TestFileTreeDirectoryOpenRequestPreservesUnsavedDocument(t *testing.T) {
	base := isolateConfig(t)
	first := filepath.Join(base, "一组")
	second := filepath.Join(base, "二组")
	for _, dir := range []string{first, second} {
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	a := New(first)
	a.toggleSource()
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Type("未保存的草稿")
	tr.Key(ui.Cmd|ui.Shift, ui.KeyF)
	if !a.treeSearch.open || !a.dirty {
		t.Fatal("the fixture did not enter workspace search with unsaved edits")
	}
	d := desktopWindows{active: a, order: []*App{a}}
	if got := d.activateFile(second); got != a {
		t.Fatal("the directory request did not reuse the active workspace window")
	}
	tr.Frame()
	if a.root != second || a.treeSearch.open || !workspaceTestFocused(a, tr, "二组") || a.path != "" || !a.dirty ||
		!strings.Contains(a.markdownSource(), "未保存的草稿") {
		t.Fatal("mounting the new tree changed the unsaved editor or left stale search state")
	}
}

func TestFileTreeWorkspaceSearchUsesHierarchyAndRevealsExactVisualMatch(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	visible, hidden := filepath.Join(root, "章节"), filepath.Join(root, ".private")
	for _, dir := range []string{visible, hidden} {
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	path := filepath.Join(visible, "Meeting.md")
	for path, body := range map[string]string{
		path:                                "# 会议\r\n\r\n今天讨论 **Needle** 的细节。\r\n",
		filepath.Join(hidden, "Secret.md"):  "Needle from an excluded directory\n",
		filepath.Join(root, "Overview.md"):  "Needles is plural\n",
		filepath.Join(root, "Checklist.md"): "Unrelated notes\n",
	} {
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(filepath.Join(root, "Overview.md"))
	a.hideSide = true
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(ui.Cmd|ui.Shift, ui.KeyF)
	if !tr.Focused("工作区搜索") || a.hideSide {
		t.Fatal("the workspace search shortcut did not focus the sidebar search")
	}
	tr.Type("needle")
	workspaceTestClick(t, tr, "整词")
	waitForWorkspaceSearch(t, a, tr)
	if len(a.treeSearch.result.hits) != 1 || !tr.HasText("章节") || tr.HasText("Secret.md") {
		t.Fatalf("search hierarchy or filter mismatch: %+v", a.treeSearch.result)
	}
	workspaceTestClick(t, tr, "区分大小写")
	waitForWorkspaceSearch(t, a, tr)
	if len(a.treeSearch.result.hits) != 0 {
		t.Fatal("case-sensitive search matched a different case")
	}
	workspaceTestClick(t, tr, "区分大小写")
	waitForWorkspaceSearch(t, a, tr)
	hit := a.treeSearch.result.hits[0]
	saveWorkspaceSnapshot(t, tr, "16-workspace-search")
	workspaceTestClick(t, tr, "3   "+hit.excerpt)
	if a.path != path || a.source != nil || a.core.SelectedDocument().PlainText() != "Needle" {
		t.Fatalf("click did not select the formatted match: %q, source=%v, selection=%q", a.path, a.source != nil, a.core.SelectedDocument().PlainText())
	}
	tr.Key(0, ui.KeyEscape)
	if a.treeSearch.open || !workspaceTestFocused(a, tr, "Meeting.md") || !a.treeView.Open.Has(visible) {
		t.Fatal("Escape did not reveal the current file in the ordinary directory tree")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyF)
	workspaceTestClick(t, tr, "文件名")
	workspaceTestClick(t, tr, "整词")
	workspaceTestClick(t, tr, "工作区搜索")
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("Checklist")
	tr.Advance(180 * time.Millisecond)
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("Meet")
	waitForWorkspaceSearch(t, a, tr)
	if hits := a.treeSearch.result.hits; len(hits) != 1 || hits[0].line != 0 || filepath.Base(hits[0].relative) != "Meeting.md" {
		t.Fatalf("stale results replaced the latest filename query: %+v", hits)
	}
}

func TestFileTreeWorkspaceSearchFindsUnsavedTextAndSignalsInvalidRegex(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	path := filepath.Join(root, "Draft.md")
	if err := os.WriteFile(path, []byte("盘上旧正文\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	a.toggleSource()
	current := a.source
	current.edit.Select(len(current.text()), len(current.text()))
	current.edit.Insert("未保存的 Target17\n")
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(ui.Cmd|ui.Shift, ui.KeyF)
	workspaceTestClick(t, tr, "正则")
	workspaceTestClick(t, tr, "工作区搜索")
	tr.Type("[")
	waitForWorkspaceSearch(t, a, tr)
	if a.treeSearch.result.err == nil {
		t.Fatal("an invalid expression was silently accepted")
	}
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type("Target[0-9]+")
	waitForWorkspaceSearch(t, a, tr)
	if len(a.treeSearch.result.hits) != 1 {
		t.Fatalf("the live buffer was not searched: %+v", a.treeSearch.result)
	}
	tr.Key(0, ui.KeyEnter)
	if a.source != current || !a.dirty || current.edit.SelectedText() != "Target17" {
		t.Fatalf("jump to the live match lost the buffer or selected %q", current.edit.SelectedText())
	}
	if disk, err := os.ReadFile(path); err != nil || strings.Contains(string(disk), "Target17") {
		t.Fatal("search committed a draft to disk")
	}
}

func waitForWorkspaceSearch(t *testing.T, a *App, tr *ui.Tester) {
	t.Helper()
	for limit := time.Now().Add(3 * time.Second); time.Now().Before(limit); {
		tr.Advance(180 * time.Millisecond)
		if a.treeSearch.ready == nil && a.treeSearch.due.IsZero() {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("workspace search did not finish")
}

func saveWorkspaceSnapshot(t *testing.T, tr *ui.Tester, name string) {
	t.Helper()
	if dir := os.Getenv("MDX_SNAPSHOT_DIR"); dir != "" {
		out, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer out.Close()
		if err := png.Encode(out, tr.Image()); err != nil {
			t.Fatal(err)
		}
	}
}
