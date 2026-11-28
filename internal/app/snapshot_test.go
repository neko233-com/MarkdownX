package app

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// TestSnapshots renders the window in several states so the layout can be
// checked without a display. Set MDX_SNAPSHOT_DIR to write the PNGs.
func TestSnapshots(t *testing.T) {
	dir := os.Getenv("MDX_SNAPSHOT_DIR")
	if dir == "" {
		t.Skip("set MDX_SNAPSHOT_DIR to write PNG snapshots")
	}
	isolateConfig(t)
	workspace := t.TempDir()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}

	const sample = "# 旅行计划\n\n这个周末去 **杭州**，`西湖` 边上走走。\n\n## 行程\n\n- 周六：西湖\n- 周日：灵隐寺\n\n> 记得带伞。\n\n- [ ] 订酒店\n- [x] 买车票\n"

	mk := func() *App {
		a := New("")
		a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(sample, doc.Off{File: true})})
		a.root = workspace
		a.refreshFiles()
		a.recents = []string{filepath.Join(workspace, "旅行计划.md"), filepath.Join(workspace, "周报.md")}
		a.templates = append(builtinTemplates(), template{Name: "我的周报", Body: "# 周报\n", Path: filepath.Join(workspace, "我的周报.md")})
		*a.title = "旅行计划"
		a.ResetBuffers()
		return a
	}
	// The recent list only shows files that exist.
	for _, n := range []string{"旅行计划.md", "周报.md", "我的周报.md"} {
		if err := os.WriteFile(filepath.Join(workspace, n), []byte("# x\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	nestedTree := func(a *App) {
		root := filepath.Join(workspace, "写作工作区")
		chapters := filepath.Join(root, "章节", "第一部分", "第二层文件夹", "深层素材与草稿")
		if err := os.MkdirAll(chapters, 0o700); err != nil {
			t.Fatal(err)
		}
		for path, body := range map[string]string{
			filepath.Join(root, "导读.md"):        "# 导读\n",
			filepath.Join(chapters, "2-行程.md"):  sample,
			filepath.Join(chapters, "10-备忘.md"): "# 备忘\n",
		} {
			if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		a.root = root
		a.refreshFiles()
		a.markSaved()
		a.OpenFile(filepath.Join(chapters, "2-行程.md"))
	}

	documentLayout := func(a *App) {
		path := tabTestFile(t, workspace, "旅行资料.md", "# 旅行资料\n\n## 核对资料\n\n- "+strings.Repeat("核对路线、站点和预计抵达时间。", 4)+"\n- 到站后再次确认。\n\n## 地点与说明\n\n| 项目 | 资料 |\n| --- | --- |\n| 城市交通路线与站点 | `https://example.org/travel/guide/transport/local/bus-schedule.html` |\n| 车票 | "+strings.Repeat("在车站服务窗口核对换乘信息。", 6)+" |\n| 地图 | 纸质与电子版 |\n")
		a.OpenFile(path)
		a.sidebarWidth = minSidebarWidth
	}
	shots := []struct {
		name  string
		width int
		mut   func(*App)
	}{
		{"01-split-recent", 1120, func(a *App) { a.mode = ModeSplit }},
		{"02-edit", 1120, func(a *App) { a.mode = ModeEdit }},
		{"03-preview", 1120, func(a *App) { a.mode = ModePreview }},
		{"04-templates", 1120, func(a *App) { a.templateLibraryOpen = true }},
		{"05-find", 1120, func(a *App) {
			a.finding = true
			a.findQuery = "西湖"
			a.refreshMatches()
		}},
		{"06-encrypt-dialog", 1120, func(a *App) { a.EncryptSave() }},
		{"07-template-dialog", 1120, func(a *App) { a.SaveAsTemplate() }},
		{"08-narrow", 900, func(a *App) {}},
		{"09-inline-link", 1120, func(a *App) { a.openLink() }},
		{"10-typora-typing", 1120, func(a *App) {
			// A document built the way typing builds it.
			md := "# 标题\n\n- 项目一\n- [ ] 待办\n\n> 引用\n\n```go\nfmt.Println(\"hi\")\n```\n"
			a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(md, doc.Off{File: true})})
			*a.title = "用标记打字"
			a.ResetBuffers()
		}},
		{"11-source", 1120, func(a *App) { a.toggleSource() }},
		{"12-outline", 1120, func(a *App) { a.sidebarPage = sidebarOutline }},
		{"13-file-tree", 1120, nestedTree},
		{"14-file-tree-filter", 900, func(a *App) {
			a.treeFilterEdit = workspaceFilterDialog{open: true, value: workspaceFilter{Mode: workspaceCustomFiles}}
		}},
		{"15-empty-workspace", 760, func(a *App) {
			a.root = ""
			a.refreshFiles()
		}},
		{"16-narrow-dark-tree", 760, func(a *App) {
			nestedTree(a)
			a.sidebarWidth = minSidebarWidth
		}},
		{"17-quick-open-tree", 1000, func(a *App) {
			nestedTree(a)
			a.openQuickOpen()
			a.quickQuery = "行程"
		}},
		{"18-command-palette", 900, func(a *App) {
			nestedTree(a)
			a.openCommandPalette()
			a.palette.query = "视图"
		}},
		{"19-zen", 900, func(a *App) {
			nestedTree(a)
			a.toggleZen()
		}},
		{"20-document-split", 1120, func(a *App) {
			nestedTree(a)
			a.splitRight()
		}},
		{"21-narrow-dark-outline", 760, func(a *App) {
			nestedTree(a)
			a.sidebarWidth = minSidebarWidth
			a.sidebarPage = sidebarOutline
		}},
		{"22-document-table", 1120, documentLayout},
		{"23-narrow-dark-table", 760, documentLayout},
	}
	for _, s := range shots {
		a := mk()
		s.mut(a)
		scale := float32(1)
		view := a.View
		if strings.Contains(s.name, "narrow-dark") {
			scale = 1.25
			view = func(c *ui.Context) {
				c.SetTheme(ui.DarkTheme())
				a.View(c)
			}
		}
		img := ui.Render(view, s.width, 760, scale)
		if img == nil {
			t.Fatalf("%s: render returned nil", s.name)
		}
		p := filepath.Join(dir, s.name+".png")
		f, err := os.Create(p)
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(f, img); err != nil {
			f.Close()
			t.Fatal(err)
		}
		f.Close()
		t.Logf("%s -> %s", s.name, p)
	}
}
