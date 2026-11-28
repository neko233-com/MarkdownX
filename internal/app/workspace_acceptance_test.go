package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	sourceeditor "github.com/egoist/mygo/ui/editor"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestFolderTreeOpensOneEditableDocumentInPlace(t *testing.T) {
	isolateConfig(t)
	cfg := loadSettings()
	cfg.Mode, cfg.TreeOpenMode = ModeSplit, workspaceOpenTab
	saveSettings(cfg)
	root := t.TempDir()
	section := filepath.Join(root, "手册")
	if err := os.Mkdir(section, 0o700); err != nil {
		t.Fatal(err)
	}
	first := tabTestFile(t, section, "2-指南.md", "# 阅读指南\r\n\r\n原位编辑\r\n")
	second := tabTestFile(t, section, "10-清单.md", "# 检查清单\n\n一篇文档\n")
	a := New(root)
	tr := ui.NewTester(a.View, 1000, 660)
	tr.Frame()
	workspaceTestClick(t, tr, "手册")
	workspaceTestClick(t, tr, filepath.Base(first))
	assertCurrent := func(path, heading string) {
		t.Helper()
		if a.path != path || len(a.tabs) != 1 || a.split != nil || a.mode != ModeEdit || a.root != root {
			t.Fatalf("tree layout after opening %q: path %q, tabs %d, mode %d, workspace %q", path, a.path, len(a.tabs), a.mode, a.root)
		}
		if !strings.Contains(a.core.Doc.PlainText(), heading) || tr.HasText("打开的文档") ||
			tr.HasText("切换视图") || tr.HasText("右侧分栏落点") || !tr.HasText("文件目录树") {
			t.Fatalf("the workspace did not render its tree and one editable document: %q", tr.Texts())
		}
	}
	assertCurrent(first, "阅读指南")
	workspaceTestClick(t, tr, "切换 Markdown 源码")
	if a.source == nil || !strings.Contains(a.source.text(), "# 阅读指南") || !a.sidebarVisible() {
		t.Fatal("the footer source switch did not keep the current file and tree")
	}
	tr.Key(ui.Cmd, ui.KeySlash)
	tr.Frame()
	if a.source != nil {
		t.Fatal("the source shortcut did not restore the rendered document")
	}
	workspaceTestClick(t, tr, filepath.Base(second))
	assertCurrent(second, "检查清单")
	workspaceTestClick(t, tr, filepath.Base(first))
	assertCurrent(first, "阅读指南")
	if raw, err := os.ReadFile(first); err != nil || string(raw) != "# 阅读指南\r\n\r\n原位编辑\r\n" {
		t.Fatalf("file browsing changed the original bytes: %q, %v", raw, err)
	}
}

func TestCommandPaletteZenPreserveTreeAndEditInPlace(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	path := filepath.Join(root, "写作计划.md")
	if err := os.WriteFile(path, []byte("初稿"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	if !tr.HasText("写作计划.md") || !a.sidebarVisible() {
		t.Fatal("workspace tree not visible")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyP)
	tr.Frame()
	if !a.palette.open || !tr.Focused("命令面板") {
		t.Fatal("palette did not receive input focus")
	}
	tr.Type("沉浸")
	tr.Frame()
	key := "Ctrl+Shift+Enter"
	if ui.Cmd == ui.Super {
		key = "Cmd+Shift+Enter"
	}
	if !tr.HasText(key) {
		t.Fatalf("platform shortcut %q is missing from the palette", key)
	}
	tr.Key(0, ui.KeyEnter)
	tr.Frame()
	if a.palette.open || a.sidebarVisible() || !a.zenMode || a.hideSide || a.path != path {
		t.Fatal("palette command did not enter Zen with its workspace settings intact")
	}
	tr.Key(0, ui.KeyEscape)
	tr.Frame()
	if a.zenMode || !a.sidebarVisible() || !tr.HasText("写作计划.md") {
		t.Fatal("Escape did not restore the file tree")
	}
	tr.Key(ui.Cmd|ui.Shift, ui.KeyEnter)
	tr.Frame()
	if !a.zenMode || a.markdownSource() != "初稿" {
		t.Fatal("Zen shortcut changed the document")
	}
}

func TestQuickOpenKeyboardCategoriesAndHeadingJump(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("正文\n\n## 发布计划\n\n说明")
	tr := ui.NewTester(a.View, 940, 680)
	tr.Frame()
	a.openQuickOpen()
	tr.Frame()
	tr.Key(0, ui.KeyTab)
	tr.Frame()
	if a.quickCategory != quickOpened || !tr.Focused("快速打开") || len(a.quickResults()) != 1 {
		t.Fatal("Tab did not select the opened-documents category")
	}
	tr.Key(ui.Shift, ui.KeyTab)
	tr.Frame()
	tr.Key(ui.Shift, ui.KeyTab)
	tr.Frame()
	if a.quickCategory != quickHeadings {
		t.Fatal("Shift+Tab did not wrap to headings")
	}
	tr.Type("发计")
	tr.Frame()
	hits := a.quickResults()
	if len(hits) != 1 || hits[0].heading == 0 || hits[0].name != "发布计划" {
		t.Fatalf("fuzzy heading results = %+v", hits)
	}
	tr.Key(0, ui.KeyEnter)
	tr.Frame()
	if a.quickOpen || a.focus != hits[0].heading {
		t.Fatal("Enter did not jump into the heading")
	}
}

func TestSelectionCommandsInWritingAndSourceViews(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("问好。**张三**回信！还有一段。\n\nkeep")
	b := a.core.Doc.Blocks[0]
	focus := func(prefix string) {
		a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: len(prefix)}))
	}
	tr := ui.NewTester(a.View, 900, 680)
	focus("问好。张")
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyE)
	tr.Frame()
	if got := a.core.SelectedText(); got != "张三" {
		t.Fatalf("styled selection = %q", got)
	}
	focus("问好。张三回")
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyL)
	tr.Frame()
	if got := a.core.SelectedText(); got != "张三回信！" {
		t.Fatalf("sentence selection = %q", got)
	}
	tr.Key(ui.Cmd|ui.Alt|ui.Shift, ui.KeyL)
	tr.Frame()
	if got := b.Text.Text; got != "问好。还有一段。" {
		t.Fatalf("remaining text = %q", got)
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	tr.Frame()
	if !strings.Contains(a.markdownSource(), "**张三**") {
		t.Fatal("undo lost inline Markdown")
	}
	a.toggleSource()
	text := a.source.text()
	a.source.edit.SetSelections(sourceeditor.Selection{Anchor: len(text) - 2, Head: len(text) - 2})
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyL)
	tr.Frame()
	s := a.source.edit.Selection()
	if got := text[s.Start():s.End()]; got != "keep" {
		t.Fatalf("source line selection = %q", got)
	}
}

func TestTableColumnShortcutUndoAndIdentity(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("| Key | Value |\n| --- | --- |\n| A | **One** |\n")
	table := a.core.Doc.Blocks[0].Kind.(doc.TableKind).Table
	b := table.Rows[1][0].Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID}))
	tr := ui.NewTester(a.View, 940, 680)
	tr.Frame()
	tr.Key(ui.Alt, ui.KeyRight)
	tr.Frame()
	if table.Rows[1][1].Blocks[0] != b || !strings.Contains(a.markdownSource(), "| **One** | A |") {
		t.Fatalf("column order = %q", a.markdownSource())
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	tr.Frame()
	if !strings.Contains(a.markdownSource(), "| A | **One** |") {
		t.Fatalf("column undo = %q", a.markdownSource())
	}
}

func TestSaveAllAndDuplicateKeepIndependentDocuments(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	first, second := filepath.Join(root, "first.md"), filepath.Join(root, "second.md")
	for path, content := range map[string]string{first: "original", second: "other"} {
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a, b := New(first), New(second)
	manager := &desktopWindows{order: []*App{a, b}, active: a}
	a.windows, b.windows = manager, manager
	a.core.SyncText(a.core.Doc.Blocks[0].ID, "first draft")
	b.core.SyncText(b.core.Doc.Blocks[0].ID, "second draft")
	a.splitRight()
	a.saveAllDocuments()
	for path, content := range map[string]string{first: "first draft", second: "second draft"} {
		data, err := os.ReadFile(path)
		if err != nil || string(data) != content {
			t.Fatalf("%s = %q, %v", path, data, err)
		}
	}
	original := a.documentState
	a.duplicateDocument()
	if len(a.tabs) != 2 || a.path != "" || !a.dirty || a.documentState == original || a.markdownSource() != "first draft" {
		t.Fatal("duplicate did not produce a distinct unsaved tab")
	}
}
