package live

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

func newRoom(t *testing.T) (*Room, string) {
	t.Helper()
	root := t.TempDir()
	st, err := store.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	repo, err := gitstore.Open(root, "Tester", "tester@example.test")
	if err != nil {
		t.Fatal(err)
	}
	room, err := NewRoom("doc", st, repo, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	return room, root
}

func TestSaveAdvancesRevisionAndCommits(t *testing.T) {
	room, _ := newRoom(t)
	ctx := context.Background()
	out, err := room.Save(ctx, 1, "# 标题\n\n正文。\n", "Alice", "test")
	if err != nil {
		t.Fatal(err)
	}
	if !out.Changed || out.Rev != 2 || out.Commit == "" {
		t.Fatalf("out = %#v", out)
	}
	if out.GitError != "" {
		t.Fatalf("git error: %s", out.GitError)
	}
	rev, content, title, _, _ := room.Current()
	if rev != 2 || !strings.Contains(content, "正文") || title != "标题" {
		t.Fatalf("current = %d %q %q", rev, content, title)
	}
}

func TestStaleSaveMergesNonOverlappingEdits(t *testing.T) {
	room, _ := newRoom(t)
	ctx := context.Background()
	if _, err := room.Save(ctx, 1, "# 标题\n\na\nb\nc\nd\ne\n", "Alice", "test"); err != nil {
		t.Fatal(err)
	}
	if _, err := room.Save(ctx, 2, "# 标题\n\na\nb\nc\nd\ne\nf\n", "Bob", "test"); err != nil {
		t.Fatal(err)
	}
	out, err := room.Save(ctx, 2, "# 标题\n\nA\nb\nc\nd\ne\n", "Carol", "test")
	if err != nil {
		t.Fatal(err)
	}
	if out.Conflict != nil || !strings.Contains(out.Content, "A") || !strings.Contains(out.Content, "f") {
		t.Fatalf("merged = %#v", out)
	}
}

func TestConflictingEditsAreNotWritten(t *testing.T) {
	room, root := newRoom(t)
	ctx := context.Background()
	if _, err := room.Save(ctx, 1, "# 标题\n\na\nb\n", "Alice", "test"); err != nil {
		t.Fatal(err)
	}
	if _, err := room.Save(ctx, 2, "# 标题\n\na\nB\n", "Bob", "test"); err != nil {
		t.Fatal(err)
	}
	out, err := room.Save(ctx, 1, "# 标题\n\na\nC\n", "Carol", "test")
	if err != nil {
		t.Fatal(err)
	}
	if out.Conflict == nil || out.Changed {
		t.Fatalf("expected conflict, got %#v", out)
	}
	_ = root
}

func TestTooBigIsRejected(t *testing.T) {
	root := t.TempDir()
	st, _ := store.Open(root)
	repo, _ := gitstore.Open(root, "Tester", "t@example.test")
	room, _ := NewRoom("doc", st, repo, 4)
	if _, err := room.Save(context.Background(), 1, "12345", "A", "test"); err != ErrTooBig {
		t.Fatalf("err = %v", err)
	}
}

// Remote editors need the new text in the update itself. Sending only a
// revision number makes a focused, clean client apply an empty string and
// overwrite the room on its next save.
func TestUpdateBroadcastsFullContent(t *testing.T) {
	room, _ := newRoom(t)
	peer := &Client{ID: "bob", Name: "Bob", send: make(chan []byte, 8), done: make(chan struct{})}
	room.join(peer)
	<-peer.send // presence, sent by join

	content := "# 标题\n\n正文。\n"
	if _, err := room.Save(context.Background(), 1, content, "Alice", "alice"); err != nil {
		t.Fatal(err)
	}
	select {
	case raw := <-peer.send:
		var update Update
		if err := json.Unmarshal(raw, &update); err != nil {
			t.Fatal(err)
		}
		if update.Type != TypeUpdate || update.Content != content {
			t.Fatalf("update = %#v", update)
		}
	default:
		t.Fatal("no update broadcast")
	}
}
