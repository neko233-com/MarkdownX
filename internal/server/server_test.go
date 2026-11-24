package server

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/rage"
)

func testServer(t *testing.T) (*httptest.Server, string) {
	t.Helper()
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git is not installed")
	}
	root := t.TempDir()
	if err := InitRepo(root); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "note.md"), []byte("# 标题\n\n正文\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(root, "note.md", "add note"); err != nil {
		t.Fatal(err)
	}
	s := New(Config{Root: root, Token: "s3cret", MessagePrefix: "web:"})
	ts := httptest.NewServer(s.Handler())
	t.Cleanup(ts.Close)
	return ts, root
}

func call(t *testing.T, ts *httptest.Server, method, path string, body any, token string) (int, map[string]any) {
	t.Helper()
	var buf bytes.Buffer
	if body != nil {
		_ = json.NewEncoder(&buf).Encode(body)
	}
	req, err := http.NewRequest(method, ts.URL+path, &buf)
	if err != nil {
		t.Fatal(err)
	}
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	req.Header.Set("Content-Type", "application/json")
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	out := map[string]any{}
	_ = json.NewDecoder(res.Body).Decode(&out)
	return res.StatusCode, out
}

func TestAuthIsRequired(t *testing.T) {
	ts, _ := testServer(t)
	if code, _ := call(t, ts, "GET", "/api/docs", nil, ""); code != http.StatusUnauthorized {
		t.Fatalf("without a token: %d", code)
	}
	if code, _ := call(t, ts, "GET", "/api/docs", nil, "wrong"); code != http.StatusUnauthorized {
		t.Fatalf("with a wrong token: %d", code)
	}
	if code, _ := call(t, ts, "GET", "/api/docs", nil, "s3cret"); code != http.StatusOK {
		t.Fatalf("with the token: %d", code)
	}
	// The token may also arrive as a query parameter.
	req, _ := http.NewRequest("GET", ts.URL+"/api/docs?token=s3cret", nil)
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	res.Body.Close()
	if res.StatusCode != http.StatusOK {
		t.Fatalf("query token: %d", res.StatusCode)
	}
}

func TestListAndRead(t *testing.T) {
	ts, _ := testServer(t)
	code, out := call(t, ts, "GET", "/api/docs", nil, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("list: %d", code)
	}
	docs, _ := out["docs"].([]any)
	if len(docs) != 1 {
		t.Fatalf("docs = %v", out["docs"])
	}
	first := docs[0].(map[string]any)
	if first["path"] != "note.md" || first["title"] != "标题" {
		t.Fatalf("doc = %v", first)
	}

	code, doc := call(t, ts, "GET", "/api/doc?path=note.md", nil, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("read: %d", code)
	}
	if !strings.Contains(doc["content"].(string), "正文") {
		t.Fatalf("content = %v", doc["content"])
	}
	if doc["base"] == "" {
		t.Fatal("missing base hash")
	}
}

func TestWriteCommitsAndBumpsBase(t *testing.T) {
	ts, root := testServer(t)
	_, doc := call(t, ts, "GET", "/api/doc?path=note.md", nil, "s3cret")
	base := doc["base"].(string)

	code, saved := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "note.md", "content": "# 标题\n\n改过的正文\n", "base": base,
	}, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("save: %d %v", code, saved)
	}
	newBase := saved["base"].(string)
	if newBase == base {
		t.Fatal("base did not change")
	}
	if saved["content"].(string) != "# 标题\n\n改过的正文\n" {
		t.Fatalf("content = %v", saved["content"])
	}

	// A second save based on the old version is a conflict.
	code, out := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "note.md", "content": "stale", "base": base,
	}, "s3cret")
	if code != http.StatusConflict {
		t.Fatalf("stale save: %d %v", code, out)
	}
	conflict, _ := out["conflict"].(map[string]any)
	if conflict == nil || !strings.Contains(conflict["content"].(string), "改过的正文") {
		t.Fatalf("conflict = %v", out)
	}

	commits, err := History(root, "note.md", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(commits) < 2 {
		t.Fatalf("commits = %d, want at least 2", len(commits))
	}
	if !strings.HasPrefix(commits[0].Subject, "web:") {
		t.Fatalf("subject = %q", commits[0].Subject)
	}
}

func TestHistoryDiffRestore(t *testing.T) {
	ts, root := testServer(t)
	_, doc := call(t, ts, "GET", "/api/doc?path=note.md", nil, "s3cret")
	call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "note.md", "content": "# 标题\n\n第二版\n", "base": doc["base"],
	}, "s3cret")

	code, out := call(t, ts, "GET", "/api/history?path=note.md", nil, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("history: %d", code)
	}
	commits, _ := out["commits"].([]any)
	if len(commits) < 2 {
		t.Fatalf("commits = %v", out)
	}
	newest := commits[0].(map[string]any)["hash"].(string)

	code, d := call(t, ts, "GET", "/api/diff?path=note.md&commit="+newest, nil, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("diff: %d", code)
	}
	if !strings.Contains(d["diff"].(string), "第二版") {
		t.Fatalf("diff = %v", d["diff"])
	}

	// Restore the oldest commit.
	oldest := commits[len(commits)-1].(map[string]any)["hash"].(string)
	code, restored := call(t, ts, "POST", "/api/restore", map[string]any{
		"path": "note.md", "commit": oldest,
	}, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("restore: %d %v", code, restored)
	}
	if strings.Contains(restored["content"].(string), "第二版") {
		t.Fatalf("content after restore = %v", restored["content"])
	}
	onDisk, err := os.ReadFile(filepath.Join(root, "note.md"))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(onDisk), "第二版") {
		t.Fatalf("file was not restored: %s", onDisk)
	}
}

func TestPathTraversalIsRejected(t *testing.T) {
	ts, _ := testServer(t)
	for _, bad := range []string{
		"../secret.md", "/etc/passwd", "a/../../b.md", "notes.txt", "",
		"a//b.md", "C:/x.md",
	} {
		code, _ := call(t, ts, "GET", "/api/doc?path="+bad, nil, "s3cret")
		if code == http.StatusOK {
			t.Errorf("%q was accepted", bad)
		}
	}
}

func TestWriteCreatesMissingDirectories(t *testing.T) {
	ts, root := testServer(t)
	code, saved := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "团队/周报.md", "content": "# 周报\n",
	}, "s3cret")
	if code != http.StatusOK {
		t.Fatalf("create: %d %v", code, saved)
	}
	if _, err := os.Stat(filepath.Join(root, "团队", "周报.md")); err != nil {
		t.Fatalf("file missing: %v", err)
	}
}

func TestStaticPageIsServed(t *testing.T) {
	ts, _ := testServer(t)
	res, err := http.Get(ts.URL + "/")
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		t.Fatalf("page: %d", res.StatusCode)
	}
	var buf bytes.Buffer
	_, _ = buf.ReadFrom(res.Body)
	if !strings.Contains(buf.String(), "MarkdownX") {
		t.Fatal("page does not look like the editor")
	}
}

func TestTokenInURLSetsACookie(t *testing.T) {
	ts, _ := testServer(t)
	client := &http.Client{
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	res, err := client.Get(ts.URL + "/?token=s3cret")
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusSeeOther {
		t.Fatalf("status = %d", res.StatusCode)
	}
	var found bool
	for _, c := range res.Cookies() {
		if c.Name == "mdx-token" && c.Value == "s3cret" {
			found = true
		}
	}
	if !found {
		t.Fatalf("cookies = %v", res.Cookies())
	}
}

func TestServerEncryptionRoundTrip(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git is not installed")
	}
	root := t.TempDir()
	if err := InitRepo(root); err != nil {
		t.Fatal(err)
	}
	seed := []byte("# 加密测试\n\n机密内容\n")
	if err := os.WriteFile(filepath.Join(root, "secret.md"), seed, 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(root, "secret.md", "seed"); err != nil {
		t.Fatal(err)
	}

	key, _ := rage.GenerateKey()
	s := New(Config{Root: root, Token: "t", EncryptionKey: key})
	ts := httptest.NewServer(s.Handler())
	t.Cleanup(ts.Close)

	// 1. PUT → 客户端发明文，服务器加密存储（这是第一次加密机会）
	newContent := "# 加密测试\n\n改过的机密\n"
	code, saved := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "secret.md", "content": newContent,
	}, "t")
	if code != http.StatusOK {
		t.Fatalf("put: %d %v", code, saved)
	}
	if saved["content"].(string) != newContent {
		t.Fatal("客户端收到的内容不是明文")
	}
	if saved["base"].(string) == "" {
		t.Fatal("missing base")
	}

	// 磁盘上应该是密文
	onDisk, _ := os.ReadFile(filepath.Join(root, "secret.md"))
	if !rage.IsServerEncrypted(onDisk) {
		t.Fatalf("磁盘上应该是密文，实际: %q", onDisk)
	} else {
		t.Log("磁盘上是密文 ✅")
	}

	// 2. GET → 返回明文给客户端（服务器透明解密）
	code, doc := call(t, ts, "GET", "/api/doc?path=secret.md", nil, "t")
	if code != http.StatusOK {
		t.Fatalf("get: %d", code)
	}
	if doc["content"].(string) != newContent {
		t.Fatalf("客户端应该收到明文，实际: %v", doc["content"])
	}
	base := doc["base"].(string)

	// 3. 冲突检测对明文生效
	code, out := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "secret.md", "content": "stale", "base": "deadbeef",
	}, "t")
	if code != http.StatusConflict {
		t.Fatalf("conflict: %d %v", code, out)
	}

	// 4. 用正确 base 再次 PUT 成功
	code, saved2 := call(t, ts, "PUT", "/api/doc", map[string]any{
		"path": "secret.md", "content": "final", "base": base,
	}, "t")
	if code != http.StatusOK {
		t.Fatalf("good put: %d %v", code, saved2)
	}
}
