package app

import (
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestFileTreeExternalAppUsesTheSelectedFolder(t *testing.T) {
	base := isolateConfig(t)
	root := filepath.Join(base, "笔记")
	subfolder := filepath.Join(root, "项目 #100% &[计划]; 2026")
	if err := os.MkdirAll(subfolder, 0o700); err != nil {
		t.Fatal(err)
	}
	a := New(root)
	node := a.tree.at(subfolder)
	var target string
	open := func(path string) error {
		target = path
		return nil
	}
	a.openWorkspaceIn(node, open)
	if target != subfolder {
		t.Fatalf("opened %q instead of the selected subfolder %q", target, subfolder)
	}
	u, err := url.Parse(workspaceCodeURL(target))
	if err != nil {
		t.Fatal(err)
	}
	want := "/" + strings.TrimPrefix(filepath.ToSlash(subfolder), "/") + "/"
	if u.Scheme != "vscode" || u.Host != "file" || u.Path != want || u.RawQuery != "" || u.Fragment != "" {
		t.Fatalf("VS Code URI expands a folder name into something other than a path: %s", u)
	}
	a.closeWorkspaceFolder()
	target = ""
	a.openWorkspaceIn(node, open)
	if target != "" || !strings.Contains(a.status, "文件操作失败") {
		t.Fatal("a folder from the closed workspace reached the external app")
	}
}
