package api

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/neko233-com/MarkdownX/server/internal/config"
	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/live"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

type testServer struct {
	url string
	cli *http.Client
}

func newTestServer(t *testing.T, cfg config.Config) testServer {
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
	hub := live.NewHub(st, repo, cfg.MaxBytes, cfg.Origin, cfg.Dev)
	srv := httptest.NewServer(New(cfg, st, repo, hub).Handler())
	t.Cleanup(srv.Close)
	return testServer{url: srv.URL, cli: srv.Client()}
}

func call(t *testing.T, ts testServer, method, path, body, token string) (int, map[string]any) {
	t.Helper()
	var r io.Reader
	if body != "" {
		r = strings.NewReader(body)
	}
	req, err := http.NewRequest(method, ts.url+path, r)
	if err != nil {
		t.Fatal(err)
	}
	req.Header.Set("Content-Type", "application/json")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	res, err := ts.cli.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	raw, _ := io.ReadAll(res.Body)
	out := map[string]any{}
	_ = json.Unmarshal(raw, &out)
	return res.StatusCode, out
}

func TestCreateReadWriteHistoryRestore(t *testing.T) {
	cfg := config.Config{MaxBytes: 1 << 20, Dev: true, CommitName: "Tester"}
	ts := newTestServer(t, cfg)
	status, created := call(t, ts, "POST", "/api/docs", `{"id":"doc","title":"标题","content":"# 标题\n\none\n"}`, "")
	if status != http.StatusCreated {
		t.Fatalf("create = %d %#v", status, created)
	}
	hash := created["commit"].(string)

	status, read := call(t, ts, "GET", "/api/docs/doc", "", "")
	if status != 200 || !strings.Contains(read["content"].(string), "one") {
		t.Fatalf("read = %d %#v", status, read)
	}
	status, written := call(t, ts, "PUT", "/api/docs/doc", `{"base":1,"content":"# 标题\n\ntwo\n"}`, "")
	if status != 200 || written["rev"].(float64) != 2 {
		t.Fatalf("write = %d %#v", status, written)
	}
	status, history := call(t, ts, "GET", "/api/docs/doc/history", "", "")
	if status != 200 || len(history["commits"].([]any)) != 2 {
		t.Fatalf("history = %d %#v", status, history)
	}
	status, diff := call(t, ts, "GET", "/api/docs/doc/diff?to="+hash, "", "")
	if status != 200 || !strings.Contains(diff["diff"].(string), "+one") {
		t.Fatalf("diff = %d %#v", status, diff)
	}
	status, restored := call(t, ts, "POST", "/api/docs/doc/restore", `{"rev":"`+hash+`"}`, "")
	if status != 200 || restored["rev"].(float64) != 3 {
		t.Fatalf("restore = %d %#v", status, restored)
	}
	_, read = call(t, ts, "GET", "/api/docs/doc", "", "")
	if !strings.Contains(read["content"].(string), "one") {
		t.Fatalf("restore content = %#v", read)
	}
}

func TestAuthAndConflict(t *testing.T) {
	cfg := config.Config{MaxBytes: 1 << 20, Dev: false, Token: "s3cret", CommitName: "Tester"}
	ts := newTestServer(t, cfg)
	if status, _ := call(t, ts, "GET", "/api/docs", "", ""); status != http.StatusUnauthorized {
		t.Fatalf("no token status = %d", status)
	}
	if status, _ := call(t, ts, "GET", "/api/docs", "", "s3cret"); status != 200 {
		t.Fatalf("token status = %d", status)
	}
	if status, _ := call(t, ts, "POST", "/api/docs", `{"id":"c","title":"C","content":"# C\n\na\nb\n"}`, "s3cret"); status != 201 {
		t.Fatalf("create = %d", status)
	}
	if status, _ := call(t, ts, "PUT", "/api/docs/c", `{"base":1,"content":"# C\n\na\nB\n"}`, "s3cret"); status != 200 {
		t.Fatalf("first write = %d", status)
	}
	status, conflict := call(t, ts, "PUT", "/api/docs/c", `{"base":1,"content":"# C\n\na\nC\n"}`, "s3cret")
	if status != http.StatusConflict || conflict["type"] != "conflict" {
		t.Fatalf("conflict = %d %#v", status, conflict)
	}
}

func TestRejectsBadInput(t *testing.T) {
	ts := newTestServer(t, config.Config{MaxBytes: 1024, Dev: true, CommitName: "Tester"})
	cases := []struct {
		method, path, body string
		status             int
	}{
		{"POST", "/api/docs", `{"id":"../x","content":"x"}`, 400},
		{"GET", "/api/docs/missing", "", 404},
		{"PUT", "/api/docs/missing", `{"base":1,"content":"x"}`, 404},
		{"GET", "/api/docs/x/diff?to=HEAD..main", "", 400},
	}
	for _, c := range cases {
		status, _ := call(t, ts, c.method, c.path, c.body, "")
		if status != c.status {
			t.Errorf("%s %s = %d, want %d", c.method, c.path, status, c.status)
		}
	}
}

func TestReadBodyLimit(t *testing.T) {
	ts := newTestServer(t, config.Config{MaxBytes: 1024, Dev: true, CommitName: "Tester"})
	big := bytes.Repeat([]byte("x"), 9000)
	body, _ := json.Marshal(map[string]string{"id": "big", "content": string(big)})
	status, _ := call(t, ts, "POST", "/api/docs", string(body), "")
	if status != http.StatusRequestEntityTooLarge && status != http.StatusBadRequest {
		t.Fatalf("big body = %d", status)
	}
}
