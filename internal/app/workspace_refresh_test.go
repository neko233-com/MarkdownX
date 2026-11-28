package app

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestFileTreeFollowsExternalFolderRenameWithUnsavedDocument(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	before := filepath.Join(root, "原目录")
	after := filepath.Join(root, "新目录")
	nested := filepath.Join("子目录", "记录.md")
	oldFile, newFile := filepath.Join(before, nested), filepath.Join(after, nested)
	if err := os.MkdirAll(filepath.Dir(oldFile), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(oldFile, []byte(utf8BOM+"正文\r\n\r\n第二段\r\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	a.OpenFile(oldFile)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	box := a.bufs[a.core.Doc.Blocks[0].ID].bounds
	tr.ClickAt(box.X+12, box.Y+box.H/2)
	tr.Key(0, ui.KeyEnd)
	tr.Type("一")
	a.Save() // Saving replaces the file on disk before the external rename.
	if a.dirty {
		t.Fatalf("the first save failed: %s", a.status)
	}
	tr.Type("二")
	mods := ui.Cmd | ui.Shift
	if runtime.GOOS == "darwin" {
		mods = ui.Cmd | ui.Ctrl
	}
	tr.Key(mods, ui.Key3)
	if !workspaceTestFocused(a, tr, "记录.md") {
		t.Fatal("the tree did not select the active document")
	}
	if err := os.Rename(before, after); err != nil {
		t.Fatal(err)
	}
	workspaceRefreshAfterChange(t, a, tr, root)
	if a.path != newFile || a.file.path != newFile || a.tree == nil || a.tree.path != root {
		t.Fatalf("the editor or workspace lost the moved file: %q, %q", a.path, a.file.path)
	}
	if !a.treeView.Open.Has(after) || !a.treeView.Open.Has(filepath.Dir(newFile)) ||
		!workspaceTestFocused(a, tr, "记录.md") || a.treeView.selectedPath() != newFile {
		t.Fatalf("the expanded branch or keyboard selection did not follow: %q", a.treeView.selectedPath())
	}
	if !a.dirty || a.markdownSource() != "正文一二\n\n第二段\n" {
		t.Fatalf("following the move altered the unsaved document: %q", a.markdownSource())
	}
	if s := loadSettings(); s.Last != newFile || slices.Contains(s.Recent, oldFile) {
		t.Fatalf("the saved document history still points to the old path: %q, %q", s.Last, s.Recent)
	}
	if b, err := os.ReadFile(newFile); err != nil || string(b) != utf8BOM+"正文一\r\n\r\n第二段\r\n" {
		t.Fatalf("tree refresh wrote unsaved content to disk: %q (%v)", b, err)
	}
	a.Save()
	if b, err := os.ReadFile(newFile); err != nil || a.dirty || string(b) != utf8BOM+"正文一二\r\n\r\n第二段\r\n" {
		t.Fatalf("save did not follow the new path or preserve encoding: %q (%v)", b, err)
	}
	if _, err := os.Stat(oldFile); !os.IsNotExist(err) {
		t.Fatalf("the old file unexpectedly reappeared: %v", err)
	}
}

func TestFileTreeFollowsExternalMoveOfKeyboardSelectionThenFallsBackToParent(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	left, right := filepath.Join(root, "1-源目录"), filepath.Join(root, "2-目标目录")
	for _, path := range []string{left, right} {
		if err := os.Mkdir(path, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	src, dst := filepath.Join(left, "原条目.md"), filepath.Join(right, "移动条目.md")
	if err := os.WriteFile(src, []byte("项目资料\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	tr.Key(0, ui.KeyDown)
	tr.Key(0, ui.KeyRight)
	tr.Key(0, ui.KeyRight)
	if !workspaceTestFocused(a, tr, "原条目.md") || a.path != "" || a.treeView.Open.Has(right) {
		t.Fatal("the fixture did not focus the file with its destination folded")
	}
	if err := os.Rename(src, dst); err != nil {
		t.Fatal(err)
	}
	workspaceRefreshAfterChange(t, a, tr, left, right)
	if !workspaceTestFocused(a, tr, "移动条目.md") || !a.treeView.Open.Has(right) || a.path != "" {
		t.Fatal("the moved keyboard selection did not follow into the folded destination")
	}
	if err := os.Remove(dst); err != nil {
		t.Fatal(err)
	}
	workspaceRefreshAfterChange(t, a, tr, right)
	if !workspaceTestFocused(a, tr, "2-目标目录") || a.path != "" {
		t.Fatalf("removing the selected file did not return keyboard focus to its parent: %q", a.treeView.selectedPath())
	}
}

func TestFileTreeKeepsOtherFoldersExpandedAndFollowsCaseOnlyRename(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	before, after := filepath.Join(root, "Before"), filepath.Join(root, "After")
	oldFile, newFile := filepath.Join(root, "Guide.md"), filepath.Join(root, "guide.md")
	if err := os.Mkdir(before, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, p := range []string{oldFile, filepath.Join(before, "Reference.md")} {
		if err := os.WriteFile(p, []byte("Sample\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(oldFile)
	tr := ui.NewTester(a.View, 960, 680)
	tr.Frame()
	workspaceTestClick(t, tr, "Before")
	workspaceTestClick(t, tr, "Guide.md")
	if !workspaceTestFocused(a, tr, "Guide.md") || !tr.HasText("Reference.md") {
		t.Fatal("the fixture did not select the document and expand the unrelated folder")
	}
	if err := os.Rename(before, after); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(oldFile, newFile); err != nil {
		t.Fatal(err)
	}
	workspaceRefreshAfterChange(t, a, tr, root)
	if a.path != newFile || !workspaceTestFocused(a, tr, "guide.md") {
		t.Fatalf("a case-only rename did not update the displayed path and selection: %q", a.path)
	}
	if !a.treeView.Open.Has(after) || !tr.HasText("Reference.md") || a.treeView.Open.Has(before) {
		t.Fatal("renaming an expanded folder lost its children when another item was selected")
	}
}

func TestFileTreeExternalMovesKeepSaveConflictsAndIgnoreUnprovenMatches(t *testing.T) {
	for _, scenario := range []string{"renamed and changed", "unrelated copy", "two hard links", "original path reused"} {
		t.Run(scenario, func(t *testing.T) {
			isolateConfig(t)
			root := t.TempDir()
			oldFile, newFile := filepath.Join(root, "start.md"), filepath.Join(root, "moved.md")
			const original = "Original content\n"
			const changed = "External writer changed the file\n"
			write := func(p, s string) {
				t.Helper()
				if err := os.WriteFile(p, []byte(s), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			write(oldFile, original)
			a := New(oldFile)
			tr := ui.NewTester(a.View, 960, 680)
			tr.Frame()
			box := a.bufs[a.core.Doc.Blocks[0].ID].bounds
			tr.ClickAt(box.X+12, box.Y+box.H/2)
			tr.Key(0, ui.KeyEnd)
			tr.Type(" local edit")
			wantPath, wantDisk := oldFile, original
			if scenario == "unrelated copy" {
				write(newFile, original)
				if err := os.Remove(oldFile); err != nil {
					t.Fatal(err)
				}
			} else if err := os.Rename(oldFile, newFile); err != nil {
				t.Fatal(err)
			}
			switch scenario {
			case "renamed and changed":
				write(newFile, changed)
				wantPath, wantDisk = newFile, changed
			case "two hard links":
				if err := os.Link(newFile, filepath.Join(root, "alias.md")); err != nil {
					t.Skipf("the temporary volume does not support hard links: %v", err)
				}
			case "original path reused":
				write(oldFile, changed)
			}
			workspaceTestClick(t, tr, "文件夹操作")
			if err := tr.ChooseMenuItem("刷新文件树"); err != nil {
				t.Fatal(err)
			}
			if a.path != wantPath || !a.dirty || a.markdownSource() != "Original content local edit\n" {
				t.Fatalf("refresh lost or incorrectly moved the local edit: %q, %q", a.path, a.markdownSource())
			}
			a.Save()
			if !a.dirty || !strings.HasPrefix(a.status, "保存失败") {
				t.Fatalf("save accepted the unresolved disk change: %q", a.status)
			}
			if b, err := os.ReadFile(newFile); err != nil || string(b) != wantDisk {
				t.Fatalf("the moved file was overwritten: %q (%v)", b, err)
			}
		})
	}
}

func workspaceRefreshAfterChange(t *testing.T, a *App, tr *ui.Tester, directories ...string) {
	t.Helper()
	for _, dir := range directories {
		stamp := a.treeNodes[dir].info.ModTime().Add(3 * time.Second)
		if err := os.Chtimes(dir, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	tr.Advance(1250 * time.Millisecond)
}
