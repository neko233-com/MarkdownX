package app

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

func TestSidebarSwitchesBetweenFileTreeAndLiveDocumentOutline(t *testing.T) {
	root := isolateConfig(t)
	chapter := filepath.Join(root, "手册")
	if err := os.Mkdir(chapter, 0o700); err != nil {
		t.Fatal(err)
	}
	first := tabTestFile(t, chapter, "指南.md", "# 阅读指南\n\n## 配置\n\n"+strings.Repeat("这里是正文。\n\n", 45)+"### 配置\n\n需要跳到这里。\n")
	second := tabTestFile(t, chapter, "清单.md", "# 交付清单\n\n## 最后检查\n")
	a := New(root)
	tr := ui.NewTester(a.View, 1000, 650)
	workspaceTestClick(t, tr, "手册")
	workspaceTestClick(t, tr, filepath.Base(first))
	doc := a.documentTab
	tree := a.treeView

	assertOutline := func() {
		t.Helper()
		if _, found := tr.AccessByRole(ui.RoleList, "文档大纲"); !found || a.sidebarPage != sidebarOutline || a.commandDialogOpen() {
			t.Fatal("文档大纲没有显示在侧栏内")
		}
		if tr.HasText("文件目录树") {
			t.Fatal("文件树和文档大纲不应同时显示")
		}
	}
	if sw, ok := tr.AccessByRole(ui.RoleButton, "切换到大纲"); !ok || sw.Bounds.X > 40 || sw.Bounds.Y < 600 {
		t.Fatalf("大纲切换按钮没有放在左下角：%+v", sw.Bounds)
	}
	workspaceTestClick(t, tr, "切换到大纲")
	assertOutline()
	headings := a.core.Doc.Headings(markdown.Slug)
	var repeat []ui.Rect
	for _, node := range tr.AccessAll() {
		if node.Role == ui.RoleListItem && node.Name == "配置" && !node.Offscreen {
			repeat = append(repeat, node.Bounds)
		}
	}
	if len(repeat) != 2 || repeat[1].Y <= repeat[0].Y {
		t.Fatalf("同名标题没有按原文顺序独立显示：%+v", repeat)
	}
	last := repeat[1]
	tr.ClickAt(last.X+last.W/2, last.Y+last.H/2)
	tr.Frame()
	assertOutline()
	if a.focus != headings[2].Block || a.editScroll.Y <= 0 || a.documentTab != doc || len(a.tabs) != 1 {
		t.Fatal("点击同名标题没有在当前正文中滚动到正确位置")
	}

	tr.Key(0, ui.KeyEnd)
	tr.Type("修订")
	tr.Frame()
	if _, found := tr.AccessByRole(ui.RoleListItem, "配置修订"); !found {
		t.Fatal("编辑正文标题后大纲没有更新")
	}
	workspaceTestClick(t, tr, "切换到文件树")
	if a.treeView != tree || !tree.Open.Has(chapter) || tree.selectedPath() != first || !workspaceHasTreeRow(tr, filepath.Base(first)) {
		t.Fatal("切回文件树后目录展开状态或当前文件选择丢失")
	}
	if !a.dirty || !strings.Contains(a.markdownSource(), "### 配置修订") {
		t.Fatal("切换侧栏改变了未保存的编辑")
	}
	tr.Key(ui.Cmd, ui.KeyS)
	workspaceTestClick(t, tr, filepath.Base(second))
	outlineTab, found := tr.AccessByRole(ui.RoleTab, "大纲")
	if !found {
		t.Fatal("侧栏顶部缺少大纲标签")
	}
	tr.ClickAt(outlineTab.Bounds.X+outlineTab.Bounds.W/2, outlineTab.Bounds.Y+outlineTab.Bounds.H/2)
	assertOutline()
	if _, found := tr.AccessByRole(ui.RoleListItem, "交付清单"); !found || a.path != second || tr.HasText("阅读指南") {
		t.Fatal("切换文件后显示了旧文档的大纲")
	}
	if s := loadSettings(); s.SidebarPage != sidebarOutline || s.HideSidebar {
		t.Fatal("大纲偏好没有保存")
	}
	workspaceTestClick(t, tr, "隐藏侧栏")
	if a.sidebarVisible() || tr.HasText("文档大纲") {
		t.Fatal("隐藏按钮没有收起大纲")
	}
	workspaceTestClick(t, tr, "显示侧栏")
	assertOutline()
	restarted := ui.NewTester(New(second).View, 1000, 650)
	if _, found := restarted.AccessByRole(ui.RoleList, "文档大纲"); !found {
		t.Fatal("重启没有恢复上次选中的侧栏页")
	}
}

func TestOutlineKeyboardNavigatesAndUpdatesSource(t *testing.T) {
	root := isolateConfig(t)
	raw := "# 开始\n\n## 下一节\n\n```md\n# 代码内容\n```\n"
	file := tabTestFile(t, root, "说明.md", raw)
	a := New(file)
	tr := ui.NewTester(a.View, 920, 660)
	tr.Key(ui.Cmd|ui.Shift, ui.KeyL)
	tr.Key(ui.Cmd, ui.KeySlash)
	mods := ui.Cmd | ui.Shift
	if runtime.GOOS == "darwin" {
		mods = ui.Cmd | ui.Ctrl
	}
	tr.Key(mods, ui.Key1)
	if a.source == nil || a.sidebarPage != sidebarOutline || !a.sidebarVisible() || !tr.Focused("文档大纲") {
		t.Fatal("大纲快捷键没有打开侧栏并把焦点放到标题列表")
	}
	if _, found := tr.AccessByRole(ui.RoleListItem, "代码内容"); found {
		t.Fatal("代码围栏里的 Markdown 被识别为文档标题")
	}
	tr.Key(0, ui.KeyDown)
	tr.Key(0, ui.KeyEnter)
	if at := a.source.edit.Selection().Head; at != strings.Index(raw, "## 下一节") {
		t.Fatalf("回车没有定位到源码里的目标标题：%d", at)
	}

	a.source.edit.Select(len(raw), len(raw))
	tr.Frame()
	tr.Type("\n### 新增标题")
	tr.Frame()
	node, found := tr.AccessByRole(ui.RoleListItem, "新增标题")
	if !found || node.Offscreen {
		t.Fatal("源码中新增的标题没有实时出现在大纲")
	}
	if got, err := os.ReadFile(file); err != nil || string(got) != raw {
		t.Fatalf("源码浏览时改动了磁盘文件：%q，%v", got, err)
	}
	tr.Key(mods, ui.Key3)
	if a.sidebarPage != sidebarFiles || !workspaceTestFocused(a, tr, "说明.md") || a.source == nil {
		t.Fatal("文件树快捷键没有恢复当前文件选择和源码视图")
	}
}

func TestOutlineWithoutHeadingsKeepsFileSwitchAvailable(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("只有正文")
	tr := ui.NewTester(a.View, 760, 620)
	workspaceTestClick(t, tr, "切换到大纲")
	if !tr.HasText("文档还没有标题") || a.outline.focus {
		t.Fatal("无标题文档没有可读的空状态")
	}
	workspaceTestClick(t, tr, "切换到文件树")
	if a.sidebarPage != sidebarFiles || !tr.HasText("打开文件夹…") {
		t.Fatal("无工作区时无法恢复文件树入口")
	}
}
