package app

import (
	"os"
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

// isolateConfig points the per-user config dir at a temp folder for a test.
func isolateConfig(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	t.Setenv("AppData", dir)
	t.Setenv("XDG_CONFIG_HOME", dir)
	t.Setenv("HOME", dir)
	return dir
}

func TestPushRecentMovesToFrontAndDedupes(t *testing.T) {
	rec := pushRecent(nil, "/a.md")
	rec = pushRecent(rec, "/b.md")
	rec = pushRecent(rec, "/a.md")
	if len(rec) != 2 {
		t.Fatalf("len = %d, want 2 (%v)", len(rec), rec)
	}
	if rec[0] != "/a.md" || rec[1] != "/b.md" {
		t.Fatalf("order = %v", rec)
	}
}

func TestPushRecentCapsTheList(t *testing.T) {
	var rec []string
	for i := 0; i < recentLimit+5; i++ {
		rec = pushRecent(rec, "/doc"+itoa(i)+".md")
	}
	if len(rec) != recentLimit {
		t.Fatalf("len = %d, want %d", len(rec), recentLimit)
	}
	if rec[0] != "/doc24.md" {
		t.Fatalf("first = %q", rec[0])
	}
}

func TestSaveTemplateRoundTrips(t *testing.T) {
	isolateConfig(t)
	p, err := saveTemplate("季度/回顾", "# 季度回顾\n\n- 要点\n")
	if err != nil {
		t.Fatal(err)
	}
	if got := p[strings.LastIndexAny(p, `/\`)+1:]; got != "季度-回顾.md" {
		t.Fatalf("file name = %q", got)
	}
	var found *template
	for _, tpl := range loadTemplates() {
		if tpl.Path == p {
			t := tpl
			found = &t
		}
	}
	if found == nil {
		t.Fatal("模板没有出现在 loadTemplates 里")
	}
	if found.Name != "季度-回顾" {
		t.Fatalf("name = %q", found.Name)
	}
	if !strings.Contains(found.Body, "要点") {
		t.Fatalf("body = %q", found.Body)
	}
	if err := os.Remove(p); err != nil {
		t.Fatal(err)
	}
}

func TestSaveTemplateStripsIllegalChars(t *testing.T) {
	isolateConfig(t)
	p, err := saveTemplate(`周报: 2026?`, "x")
	if err != nil {
		t.Fatal(err)
	}
	base := p[strings.LastIndexAny(p, `/\`)+1:]
	if strings.ContainsAny(base, `:*?`) {
		t.Fatalf("file name still holds illegal chars: %q", base)
	}
	if base != "周报- 2026-.md" {
		t.Fatalf("file name = %q", base)
	}
}

func TestInsertTemplateAppendsAndUndoes(t *testing.T) {
	isolateConfig(t)
	a := New("")
	a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks("existing", doc.Off{File: true})})
	a.ResetBuffers()
	before := len(a.core.Doc.Blocks)

	a.InsertTemplate(builtinTemplates()[0])
	if len(a.core.Doc.Blocks) <= before {
		t.Fatalf("blocks = %d, want more than %d", len(a.core.Doc.Blocks), before)
	}
	if !a.dirty {
		t.Fatal("插入模板后应标记为未保存")
	}
	md := doc.Export(a.core.Doc, doc.Options{}).Text
	if !strings.Contains(md, "会议记录") {
		t.Fatalf("导出内容缺少模板正文:\n%s", md)
	}

	if !a.core.Undo() {
		t.Fatal("插入模板应可以撤销")
	}
	if got := len(a.core.Doc.Blocks); got != before {
		t.Fatalf("撤销后 blocks = %d, want %d", got, before)
	}
}

func TestImportTemplateFromMarkdownFile(t *testing.T) {
	dir := isolateConfig(t)
	src := dir + string(os.PathSeparator) + "现有笔记.md"
	if err := os.WriteFile(src, []byte("# 现有笔记\n\n正文内容\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	a := New("")
	if err := a.ImportTemplateFile(src); err != nil {
		t.Fatal(err)
	}
	if !a.templateLibraryOpen {
		t.Fatal("导入模板后应打开模板库")
	}
	var found *template
	for _, tpl := range a.templates {
		if tpl.Name == "现有笔记" {
			tpl := tpl
			found = &tpl
		}
	}
	if found == nil {
		t.Fatalf("导入的模板不在列表里：%v", a.templates)
	}
	if !strings.Contains(found.Body, "正文内容") {
		t.Fatalf("内容没有备份下来:\n%s", found.Body)
	}
	if found.Path == "" || found.Path == src {
		t.Fatalf("应写入模板目录而不是原文件：%q", found.Path)
	}
}

func TestDeleteTemplateRemovesOnlyUserTemplates(t *testing.T) {
	isolateConfig(t)
	p, err := saveTemplate("待删模板", "x")
	if err != nil {
		t.Fatal(err)
	}
	a := New("")
	a.DeleteTemplate(template{Name: "会议记录"}) // 内置：无路径，应被忽略
	a.DeleteTemplate(template{Name: "待删模板", Path: p})
	if _, err := os.Stat(p); !os.IsNotExist(err) {
		t.Fatalf("模板文件没有被删除：%v", err)
	}
	for _, tpl := range a.templates {
		if tpl.Name == "待删模板" {
			t.Fatal("删除后模板仍在列表里")
		}
	}
}

func TestInsertTemplateEmpty(t *testing.T) {
	isolateConfig(t)
	a := New("")
	before := len(a.core.Doc.Blocks)
	a.InsertTemplate(template{Name: "空", Body: "\n\n"})
	if len(a.core.Doc.Blocks) != before {
		t.Fatal("空模板不应插入任何块")
	}
	if !strings.Contains(a.status, "为空") {
		t.Fatalf("status = %q", a.status)
	}
}
