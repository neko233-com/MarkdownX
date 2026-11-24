package app

import (
	"image/png"
	"os"
	"path/filepath"
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
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}

	const sample = "# 旅行计划\n\n这个周末去 **杭州**，`西湖` 边上走走。\n\n## 行程\n\n- 周六：西湖\n- 周日：灵隐寺\n\n> 记得带伞。\n\n- [ ] 订酒店\n- [x] 买车票\n"

	mk := func() *App {
		a := New("")
		a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(sample, doc.Off{File: true})})
		a.root = dir
		a.files = []string{"周报.md", "读书笔记.md"}
		a.recents = []string{filepath.Join(dir, "旅行计划.md"), filepath.Join(dir, "周报.md")}
		a.templates = append(builtinTemplates(), template{Name: "我的周报", Body: "# 周报\n", Path: filepath.Join(dir, "我的周报.md")})
		*a.title = "旅行计划"
		a.ResetBuffers()
		return a
	}
	// The recent list only shows files that exist.
	for _, n := range []string{"旅行计划.md", "周报.md", "我的周报.md"} {
		if err := os.WriteFile(filepath.Join(dir, n), []byte("# x\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	shots := []struct {
		name  string
		width int
		mut   func(*App)
	}{
		{"01-split-recent", 1120, func(a *App) {}},
		{"02-edit", 1120, func(a *App) { a.mode = ModeEdit }},
		{"03-preview", 1120, func(a *App) { a.mode = ModePreview }},
		{"04-templates", 1120, func(a *App) { a.sideTab = 1 }},
		{"05-find", 1120, func(a *App) {
			a.finding = true
			a.findQuery = "西湖"
			a.refreshMatches()
		}},
		{"06-encrypt-dialog", 1120, func(a *App) { a.EncryptSave() }},
		{"07-template-dialog", 1120, func(a *App) { a.SaveAsTemplate() }},
		{"08-narrow", 900, func(a *App) {}},
		{"09-link-dialog", 1120, func(a *App) { a.openLink() }},
		{"10-typora-typing", 1120, func(a *App) {
			// A document built the way typing builds it.
			md := "# 标题\n\n- 项目一\n- [ ] 待办\n\n> 引用\n\n```go\nfmt.Println(\"hi\")\n```\n"
			a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(md, doc.Off{File: true})})
			*a.title = "用标记打字"
			a.ResetBuffers()
		}},
	}
	for _, s := range shots {
		a := mk()
		s.mut(a)
		img := ui.Render(a.View, s.width, 760, 1)
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
