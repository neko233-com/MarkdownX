package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// The open file is visible in the workspace tree and recorded in recents.
func TestOpeningAFileShowsItInRecents(t *testing.T) {
	dir := isolateConfig(t)
	p := filepath.Join(dir, "笔记.md")
	if err := os.WriteFile(p, []byte("# 笔记\n\n内容\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	a := New("")
	a.OpenFile(p)
	if len(a.recents) == 0 || !samePath(a.recents[0], p) {
		t.Fatalf("recents = %v", a.recents)
	}
	tr := ui.NewTester(a.View, 1120, 760)
	tr.Frame()
	if _, ok := tr.Find("笔记.md"); !ok {
		t.Fatal("the opened file was not visible in the tree")
	}
}

// TestTemplateFlowThroughTheUI drives the native template library,
// insert a template, and save the document as a new one.
func TestTemplateFlowThroughTheUI(t *testing.T) {
	isolateConfig(t)
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks("existing", doc.Off{File: true})})
	a.ResetBuffers()

	tr := ui.NewTester(a.View, 1120, 760)
	tr.Frame()

	for _, cmd := range commands {
		if cmd.label == "模板…" {
			cmd.run(a)
			break
		}
	}
	tr.Frame()
	if !tr.HasText("文档模板") {
		t.Fatal("template library menu did not open the dialogue")
	}

	before := len(a.core.Doc.Blocks)
	if err := tr.Click("会议记录"); err != nil {
		t.Fatalf("click template: %v", err)
	}
	if len(a.core.Doc.Blocks) <= before {
		t.Fatalf("blocks = %d, want more than %d", len(a.core.Doc.Blocks), before)
	}

	if err := tr.Click("存为模板"); err != nil {
		t.Fatalf("click save-as-template: %v", err)
	}
	if !a.tmplOpen {
		t.Fatal("存为模板应打开命名对话框")
	}

	a.tmplName = "A 测试模板"
	tr.Frame()
	if err := tr.Click("保存模板"); err != nil {
		t.Fatalf("click confirm: %v", err)
	}
	if a.tmplOpen {
		t.Fatal("确认后对话框应关闭")
	}

	var found *template
	for _, tpl := range a.templates {
		if tpl.Name == "A 测试模板" {
			tpl := tpl
			found = &tpl
		}
	}
	if found == nil {
		t.Fatalf("模板没有出现在列表里：%v", a.templates)
	}
	if _, err := os.Stat(found.Path); err != nil {
		t.Fatalf("模板文件不存在：%v", err)
	}
	if !strings.Contains(found.Body, "会议记录") {
		t.Fatalf("模板内容应包含刚才插入的内容:\n%s", found.Body)
	}

	// The new template has a delete button; using it drops the file.
	if err := tr.Click("删除模板"); err != nil {
		t.Fatalf("click delete: %v", err)
	}
	if _, err := os.Stat(found.Path); !os.IsNotExist(err) {
		t.Fatalf("删除后文件仍在：%v", err)
	}
}
