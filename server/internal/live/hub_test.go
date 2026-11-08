package live

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gorilla/websocket"
	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

func TestWebSocketSnapshotAndSave(t *testing.T) {
	root := t.TempDir()
	st, err := store.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	repo, err := gitstore.Open(root, "Tester", "tester@example.test")
	if err != nil {
		t.Fatal(err)
	}
	hub := NewHub(st, repo, 1<<20, "", true)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hub.ServeWS(w, r, "doc")
	}))
	defer srv.Close()

	url := "ws" + strings.TrimPrefix(srv.URL, "http") + "/?name=Tester"
	ws, _, err := websocket.DefaultDialer.Dial(url, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer ws.Close()
	_ = ws.SetReadDeadline(time.Now().Add(5 * time.Second))

	var snap Snapshot
	if err := ws.ReadJSON(&snap); err != nil {
		t.Fatal(err)
	}
	if snap.Type != TypeSnapshot || snap.Rev != 1 {
		t.Fatalf("snapshot = %#v", snap)
	}
	if err := ws.WriteJSON(map[string]any{
		"type": "save", "base": snap.Rev, "content": "# WS\n\nhello\n",
	}); err != nil {
		t.Fatal(err)
	}
	seenSave := false
	for i := 0; i < 4; i++ {
		var msg map[string]any
		if err := ws.ReadJSON(&msg); err != nil {
			t.Fatal(err)
		}
		switch msg["type"] {
		case TypeUpdate:
			if msg["rev"].(float64) != 2 {
				t.Fatalf("update = %#v", msg)
			}
		case TypeSaved:
			seenSave = true
		}
		if seenSave {
			break
		}
	}
	if !seenSave {
		t.Fatal("no saved ack")
	}
	content, err := st.Read("doc")
	if err != nil || !strings.Contains(string(content), "hello") {
		t.Fatalf("stored = %q, %v", content, err)
	}
}
