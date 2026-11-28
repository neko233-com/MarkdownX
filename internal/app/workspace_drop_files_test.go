package app

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeMultiDropMovesWorkspaceEntriesAndCopiesExternalFiles(t *testing.T) {
	isolateConfig(t)
	base := t.TempDir()
	root := filepath.Join(base, "Workspace")
	inbox := filepath.Join(root, "Inbox")
	archive := filepath.Join(root, "Archive")
	assets := filepath.Join(inbox, "插图")
	bundle := filepath.Join(inbox, "小册")
	for _, dir := range []string{archive, assets, bundle} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	note := filepath.Join(inbox, "笔记.md")
	cover := filepath.Join(assets, "cover 1.png")
	chapter := filepath.Join(bundle, "章节.md")
	external := filepath.Join(base, "外部.md")
	const destination = "<插图/cover%201.png>"
	const original = utf8BOM + "# 记录\r\n\r\n![封面](" + destination + " '标题')\r\n"
	files := map[string]string{note: original, cover: "asset fixture", chapter: "# 原章节\n", external: "# 外部\n"}
	for path, data := range files {
		if err := os.WriteFile(path, []byte(data), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	saveSettings(settings{TreeFilter: workspaceFilter{Mode: workspaceAllFiles}})
	a := New(root)
	a.OpenFile(note)
	a.toggleSource()
	source := a.source
	at := len(source.text())
	source.edit.Select(at, at)
	source.edit.Insert("\n未保存\n")
	other := New(chapter)
	desktop := &desktopWindows{order: []*App{a, other}, active: a}
	a.windows, other.windows = desktop, desktop
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	row, ok := tr.Find("Archive")
	if !ok || !tr.DropFiles(row.X+row.W/2, row.Y+row.H/2, note, cover, chapter, bundle, external, note) {
		t.Fatal("the tree did not accept the mixed file list")
	}
	movedNote := filepath.Join(archive, filepath.Base(note))
	movedCover := filepath.Join(archive, filepath.Base(cover))
	movedChapter := filepath.Join(archive, filepath.Base(bundle), filepath.Base(chapter))
	copiedExternal := filepath.Join(archive, filepath.Base(external))
	if len(a.treeUndo.moves) != 3 || len(a.treeUndo.copies) != 1 || a.path != movedNote || other.path != movedChapter || a.source != source || !a.dirty {
		t.Fatalf("mixed drop did not keep one group and follow both editors: %q, %q, %q", a.path, other.path, a.status)
	}
	if got := source.text(); !strings.Contains(got, "<cover%201.png>") || !strings.HasSuffix(got, "未保存\n") {
		t.Fatalf("relative image or unsaved source did not follow the batch: %q", got)
	}
	for _, path := range []string{note, cover, bundle, filepath.Join(archive, filepath.Base(chapter))} {
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			t.Fatalf("a moved entry still exists or was copied twice: %q (%v)", path, err)
		}
	}
	for path, want := range map[string]string{movedCover: files[cover], movedChapter: files[chapter], copiedExternal: files[external], external: files[external]} {
		if data, err := os.ReadFile(path); err != nil || string(data) != want {
			t.Fatalf("%q = %q, want %q (%v)", path, data, want, err)
		}
	}
	a.Save()
	if a.dirty || a.file.path != movedNote {
		t.Fatalf("saving the relocated document failed: %q", a.status)
	}
	if err := os.WriteFile(copiedExternal, []byte("编辑过的副本"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := a.undoTreeOperation(); err == nil || !strings.Contains(err.Error(), "副本已有更改") {
		t.Fatalf("undo must protect a changed copy: %v", err)
	}
	if len(a.treeUndo.moves) != 0 || len(a.treeUndo.copies) != 1 || a.path != note || other.path != chapter || a.source != source {
		t.Fatalf("undo did not restore both files and leave only the changed copy pending: %q, %q", a.path, other.path)
	}
	if data, err := os.ReadFile(note); err != nil || !bytes.Equal(data, []byte(original+"\r\n未保存\r\n")) {
		t.Fatalf("undo did not restore the reference, encoding and saved edits: %q (%v)", data, err)
	}
	if data, err := os.ReadFile(copiedExternal); err != nil || string(data) != "编辑过的副本" {
		t.Fatalf("undo changed an edited copy: %q (%v)", data, err)
	}
}

func TestFileTreeBatchDropAndUndoPreserveNameCollisions(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	inbox, archive := filepath.Join(root, "Inbox"), filepath.Join(root, "Archive")
	for _, dir := range []string{inbox, archive} {
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	first := filepath.Join(inbox, "First.md")
	second := filepath.Join(inbox, "Second.md")
	blocked := filepath.Join(inbox, "Collision.md")
	occupied := filepath.Join(archive, filepath.Base(blocked))
	for path, body := range map[string]string{first: "first", second: "second", blocked: "from", occupied: "occupied"} {
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	row, ok := tr.Find("Archive")
	if !ok || !tr.DropFiles(row.X+row.W/2, row.Y+row.H/2, first, blocked, second) || len(a.treeUndo.moves) != 2 || !strings.Contains(a.status, "1 个条目未完成") {
		t.Fatalf("the drop did not keep the successful moves: %q", a.status)
	}
	if err := os.WriteFile(second, []byte("newer"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := a.undoTreeOperation(); !errors.Is(err, os.ErrExist) || len(a.treeUndo.moves) != 2 {
		t.Fatalf("undo should preserve the batch while an old path is occupied: %v", err)
	}
	movedFirst := filepath.Join(archive, filepath.Base(first))
	if data, err := os.ReadFile(movedFirst); err != nil || string(data) != "first" {
		t.Fatalf("undo partially moved an unaffected item: %q (%v)", data, err)
	}
	newer := filepath.Join(inbox, "Retained.md")
	if err := os.Rename(second, newer); err != nil {
		t.Fatal(err)
	}
	if err := a.undoTreeOperation(); err != nil || a.treeUndo.available() {
		t.Fatalf("undo could not be retried: %v", err)
	}
	for path, want := range map[string]string{first: "first", second: "second", blocked: "from", occupied: "occupied", newer: "newer"} {
		if data, err := os.ReadFile(path); err != nil || string(data) != want {
			t.Fatalf("%q after undo = %q, want %q (%v)", path, data, want, err)
		}
	}
	tr.Frame()
	row, ok = tr.Find("Archive")
	tr.Key(ui.Alt, ui.KeyUnknown)
	if !ok || !tr.DropFiles(row.X+row.W/2, row.Y+row.H/2, first, second) || len(a.treeUndo.moves) != 0 || len(a.treeUndo.copies) != 2 {
		t.Fatalf("Alt did not copy the entire batch: %q", a.status)
	}
	for path, want := range map[string]string{first: "first", second: "second", movedFirst: "first", filepath.Join(archive, filepath.Base(second)): "second"} {
		if data, err := os.ReadFile(path); err != nil || string(data) != want {
			t.Fatalf("%q after Alt drop = %q, want %q (%v)", path, data, want, err)
		}
	}
}
