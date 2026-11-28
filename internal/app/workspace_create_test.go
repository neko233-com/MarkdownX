package app

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeCreatesBesideAFileWithThePreferredType(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "笔记库")
	parent := filepath.Join(root, "项目")
	if err := os.MkdirAll(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	anchor := filepath.Join(parent, "参考.md")
	current := filepath.Join(root, "当前.md")
	occupied := filepath.Join(parent, "已存在.text")
	for _, path := range []string{anchor, current, occupied} {
		if err := os.WriteFile(path, []byte("原始内容\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(current)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyComma)
	workspaceTestClick(t, tr, ".md")
	workspaceTestClick(t, tr, ".text")
	if got := loadSettings().TreeDocumentExt; got != ".text" {
		t.Fatalf("the preference did not retain .text: %q", got)
	}
	saveWorkspaceSnapshot(t, tr, "25-workspace-create-preference")
	workspaceTestClick(t, tr, "完成")
	workspaceTestClick(t, tr, "项目")
	workspaceTestMenu(t, tr, "参考.md", "新建文件夹…")
	tr.Type("素材.txt")
	tr.Key(0, ui.KeyEnter)
	folder := filepath.Join(parent, "素材.txt")
	if got, err := os.Stat(folder); err != nil || !got.IsDir() {
		t.Fatalf("the file's sibling folder was not created: %v", err)
	}
	if !workspaceTestFocused(a, tr, "素材.txt") || a.path != current {
		t.Fatal("creating a folder did not select it or switched the document")
	}
	tr.Key(0, ui.KeyDown)
	if !workspaceTestFocused(a, tr, "参考.md") {
		t.Fatalf("the keyboard context menu fixture selected %q", a.treeView.selectedPath())
	}
	tr.Key(ui.Shift, ui.KeyF10)
	if err := tr.ChooseMenuItem("新建文件…"); err != nil {
		t.Fatal(err)
	}
	if a.treeEdit.node == nil || a.treeEdit.node.path != anchor || !tr.Focused("文件名") {
		t.Fatal("the keyboard context menu did not use the selected file as its anchor")
	}
	tr.Type("已存在")
	tr.Key(0, ui.KeyEnter)
	if !a.treeEdit.open || a.treeEdit.err == "" || a.path != current {
		t.Fatal("an existing file was accepted as the new file")
	}
	if body, err := os.ReadFile(occupied); err != nil || string(body) != "原始内容\n" {
		t.Fatalf("an existing file was changed: %q, %v", body, err)
	}
	tr.Key(0, ui.KeyEscape)
	if a.treeEdit.open || !workspaceTestFocused(a, tr, "参考.md") {
		t.Fatal("cancelling the new file dialog did not restore the selected file")
	}
	workspaceTestMenu(t, tr, "参考.md", "新建文件…")
	tr.Type("摘录")
	tr.Key(0, ui.KeyEnter)
	newFile := filepath.Join(parent, "摘录.text")
	if a.path != newFile || a.file.path != newFile || !workspaceTestFocused(a, tr, "摘录.text") {
		t.Fatalf("the .text file was not created, opened and selected: %q (%s)", a.path, a.status)
	}
	tr.Key(ui.Cmd, ui.KeySlash)
	tr.Type("保存到原扩展名")
	a.Save()
	if body, err := os.ReadFile(newFile); err != nil || string(body) != "保存到原扩展名" || a.dirty {
		t.Fatalf("saving the new .text did not update the same file: %q, %v", body, err)
	}
	workspaceTestMenu(t, tr, "摘录.text", "新建文件…")
	if !a.treeEdit.open || !tr.Focused("文件名") {
		t.Fatalf("new file did not accept the just-saved neighbor as its anchor: %s", a.status)
	}
	tr.Type("导读.Markdown")
	tr.Key(0, ui.KeyEnter)
	if want := filepath.Join(parent, "导读.Markdown"); a.path != want || !workspaceTestFocused(a, tr, "导读.Markdown") {
		t.Fatalf("an explicit supported extension was changed: %q", a.path)
	}
}

func TestFileTreeSiblingCreateDetectsAReplacedDestination(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "笔记库")
	parent := filepath.Join(root, "待整理")
	if err := os.MkdirAll(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(parent, "参考.md"), []byte("资料\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestClick(t, tr, "待整理")
	workspaceTestMenu(t, tr, "参考.md", "新建文件…")
	moved := filepath.Join(root, "已移走")
	if err := os.Rename(parent, moved); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	tr.Type("原目录里的新笔记")
	tr.Key(0, ui.KeyEnter)
	if !a.treeEdit.open || a.treeEdit.err == "" || a.path != "" {
		t.Fatal("confirming the dialog accepted a different destination")
	}
	for _, dir := range []string{parent, moved} {
		if _, err := os.Stat(filepath.Join(dir, "原目录里的新笔记.md")); !os.IsNotExist(err) {
			t.Fatalf("the failed create wrote into %s: %v", dir, err)
		}
	}
}

func TestFileTreeTemplateUsesThePreferredTypeAndDocumentName(t *testing.T) {
	root := filepath.Join(isolateConfig(t), "模板工作区")
	if err := os.Mkdir(root, 0o700); err != nil {
		t.Fatal(err)
	}
	saveSettings(settings{TreeDocumentExt: ".txt"})
	body := utf8BOM + "---\r\ntypora-template:\r\n  filename: 会议提纲\r\n---\r\n\r\n# {{title}}\r\n\r\n{{cursor}}\r\n"
	if _, err := saveTemplate("会议", body); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestMenu(t, tr, "模板工作区", "从模板新建…")
	tr.Key(0, ui.KeyEnter)
	tr.Key(0, ui.KeyEnd)
	tr.Key(0, ui.KeyEnter)
	if a.treeEdit.name != "会议提纲" {
		t.Fatalf("template filename was not suggested: %q", a.treeEdit.name)
	}
	tr.Key(0, ui.KeyTab)
	tr.Key(0, ui.KeyEnter)
	want := filepath.Join(root, "会议提纲.txt")
	if a.path != want || a.file.path != want || a.dirty || !tr.HasText("会议提纲.txt") {
		t.Fatalf("the template did not open at the preferred path: %q (%s)", a.path, a.status)
	}
	if disk, err := os.ReadFile(want); err != nil || string(disk) != utf8BOM+"\r\n# 会议提纲\r\n\r\n\r\n" {
		t.Fatalf("template title, BOM or newlines changed: %q, %v", disk, err)
	}
}
