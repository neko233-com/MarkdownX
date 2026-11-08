package gitstore

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func openTestRepo(t *testing.T) *Repo {
	t.Helper()
	root := t.TempDir()
	r, err := Open(root, "Tester", "tester@example.test")
	if err != nil {
		t.Fatal(err)
	}
	return r
}

func TestCommitLogShowAndDiff(t *testing.T) {
	r := openTestRepo(t)
	rel := "docs/a.md"
	path := filepath.Join(r.root, filepath.FromSlash(rel))
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("# A\n\none\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	hash, err := r.Commit(ctx, rel, "first", "Tester")
	if err != nil || hash == "" {
		t.Fatalf("commit = %q, %v", hash, err)
	}
	if _, err := r.Commit(ctx, rel, "again", "Tester"); !errors.Is(err, ErrNoChanges) {
		t.Fatalf("expected ErrNoChanges, got %v", err)
	}
	log, err := r.Log(ctx, rel, 10)
	if err != nil || len(log) != 1 || log[0].Hash != hash {
		t.Fatalf("log = %#v, %v", log, err)
	}
	got, err := r.Show(ctx, hash, rel)
	if err != nil || !strings.Contains(string(got), "one") {
		t.Fatalf("show = %q, %v", got, err)
	}

	if err := os.WriteFile(path, []byte("# A\n\ntwo\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := r.Commit(ctx, rel, "second", "Tester"); err != nil {
		t.Fatal(err)
	}
	diff, err := r.Diff(ctx, hash, "", rel)
	if err != nil || !strings.Contains(diff, "+two") || !strings.Contains(diff, "-one") {
		t.Fatalf("diff = %q, %v", diff, err)
	}
}

func TestMergeCleanAndConflict(t *testing.T) {
	r := openTestRepo(t)
	ctx := context.Background()
	merged, clean, err := r.Merge(ctx, []byte("a\nb\nc\nd\n"), []byte("A\nb\nc\nd\n"), []byte("a\nb\nc\nD\n"))
	if err != nil || !clean || !strings.Contains(string(merged), "A") || !strings.Contains(string(merged), "D") {
		t.Fatalf("clean merge = %q, %v, %v", merged, clean, err)
	}
	_, clean, err = r.Merge(ctx, []byte("a\nb\n"), []byte("a\nB\n"), []byte("a\nC\n"))
	if err != nil || clean {
		t.Fatalf("expected conflict, clean=%v err=%v", clean, err)
	}
}

func TestRejectsUnsafePathsAndRevisions(t *testing.T) {
	r := openTestRepo(t)
	ctx := context.Background()
	for _, rel := range []string{"../x", "/x", `docs\a.md`} {
		if _, err := r.Commit(ctx, rel, "x", "Tester"); err == nil {
			t.Errorf("Commit(%q) succeeded", rel)
		}
	}
	if _, err := r.Show(ctx, "HEAD..main", "docs/a.md"); err == nil {
		t.Error("unsafe revision accepted")
	}
}
