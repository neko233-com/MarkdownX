package app

import (
	"bytes"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestFileTreeWindowsTerminalUsesTheFolderAsWorkingDirectory(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "文档 & 活动; 计划")
	if err := os.Mkdir(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	cmd, err := workspaceTerminalCommand(dir)
	if err != nil {
		t.Fatal(err)
	}
	if cmd.Dir != dir || !filepath.IsAbs(cmd.Path) {
		t.Fatalf("terminal path is not absolute or does not start in the selected folder: %q, %q", cmd.Path, cmd.Dir)
	}
	var want []string
	switch strings.ToLower(filepath.Base(cmd.Path)) {
	case "wt.exe":
		want = []string{"new-tab", "--startingDirectory", "."}
	case "pwsh.exe":
		want = []string{"-NoLogo"}
	case "cmd.exe":
		want = []string{"/D"}
	default:
		t.Fatalf("unexpected Windows terminal program: %q", cmd.Path)
	}
	if !slices.Equal(cmd.Args[1:], want) {
		t.Fatalf("terminal got %q, want %q", cmd.Args[1:], want)
	}
}

func TestFileTreeReadsNativeHiddenAttributes(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	path := filepath.Join(root, "Hidden.md")
	if err := os.WriteFile(path, []byte("example"), 0o600); err != nil {
		t.Fatal(err)
	}
	u, err := windows.UTF16PtrFromString(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetFileAttributes(u, windows.FILE_ATTRIBUTE_HIDDEN); err != nil {
		t.Fatal(err)
	}
	tree, _ := readWorkspace(root, workspaceFilter{})
	if tree.at(path) != nil {
		t.Fatal("Windows hidden attribute was ignored")
	}
	tree, _ = readWorkspace(root, workspaceFilter{ShowHidden: true})
	if tree.at(path) == nil {
		t.Fatal("native hidden file did not appear when enabled")
	}
}

func TestFileTreeUndoAfterUnreadableRebaseRestoresOriginalPaths(t *testing.T) {
	isolateConfig(t)
	root := t.TempDir()
	path := filepath.Join(root, "Page.md")
	folder := filepath.Join(root, "Archive")
	if err := os.Mkdir(folder, 0o700); err != nil {
		t.Fatal(err)
	}
	original := []byte("![caption](./pics/shared.svg)\r\n")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	if err := os.Chmod(path, 0o444); err != nil {
		t.Fatal(err)
	}
	moved := filepath.Join(folder, "Page.md")
	t.Cleanup(func() { _ = os.Chmod(path, 0o600); _ = os.Chmod(moved, 0o600) })
	if err := a.moveTreeInto(a.tree.at(path), a.tree.at(folder)); err != nil {
		t.Fatal(err)
	}
	if len(a.treeUndo.moves) != 1 || !a.treeUndo.moves[0].images[moved].failed {
		t.Fatalf("expected a readonly file to prevent rebasing: %q", a.status)
	}
	if err := a.undoTreeOperation(); err != nil {
		t.Fatal(err)
	}
	if actual, err := os.ReadFile(path); err != nil || !bytes.Equal(actual, original) {
		t.Fatalf("undo rewrote links never changed by the move: %q, %v", actual, err)
	}
}
