package store

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWriteReadAndList(t *testing.T) {
	s, err := Open(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err := s.Write("lesson-1", []byte("# 第一课\n\n正文。\n"), 1<<20); err != nil {
		t.Fatal(err)
	}
	got, err := s.Read("lesson-1")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(got), "第一课") {
		t.Fatalf("unexpected content: %q", got)
	}
	doc, err := s.Stat("lesson-1")
	if err != nil {
		t.Fatal(err)
	}
	if doc.Title != "第一课" {
		t.Fatalf("title = %q", doc.Title)
	}
	docs, err := s.List()
	if err != nil || len(docs) != 1 {
		t.Fatalf("list = %#v, %v", docs, err)
	}
}

func TestRejectsUnsafeIDs(t *testing.T) {
	for _, id := range []string{"", "../x", "a/b", `a\b`, ".hidden", ".."} {
		if ValidID(id) {
			t.Errorf("ValidID(%q) = true", id)
		}
	}
}

func TestAtomicWriteLeavesNoTemp(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "a.md")
	if err := AtomicWrite(path, []byte("one")); err != nil {
		t.Fatal(err)
	}
	if err := AtomicWrite(path, []byte("two")); err != nil {
		t.Fatal(err)
	}
	b, _ := os.ReadFile(path)
	if string(b) != "two" {
		t.Fatalf("content = %q", b)
	}
	entries, _ := os.ReadDir(dir)
	for _, e := range entries {
		if strings.Contains(e.Name(), ".tmp") {
			t.Fatalf("left temp file %s", e.Name())
		}
	}
}
