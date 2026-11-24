package server

import (
	"crypto/subtle"
	"embed"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/neko233/markdownx/internal/rage"
)

//go:embed web/*
var webFS embed.FS

// Config is how the server runs.
type Config struct {
	// Root is the git workspace whose Markdown files are shared.
	Root string
	// Token, when set, must accompany every API call.
	Token string
	// Commit messages prefix; empty uses the default.
	MessagePrefix string
	// EncryptionKey, when set, encrypts every file on disk with a
	// server-side secretbox key (transparent to clients).
	EncryptionKey []byte
}

// Server is the collaboration server.
type Server struct {
	cfg     Config
	encKey  *[rage.KeyLen]byte
	mu      sync.Mutex
	clients map[chan Event]struct{}
}

// Event is a change notification pushed to open editors.
type Event struct {
	Path   string `json:"path"`
	Base   string `json:"base"`
	Action string `json:"action"`
	// Content rides along so a reader can apply it without a second call.
	Content string `json:"content,omitempty"`
}

// New makes a server over a workspace. Every accepted save is committed.
func New(cfg Config) *Server {
	if cfg.MessagePrefix == "" {
		cfg.MessagePrefix = "web:"
	}
	s := &Server{cfg: cfg, clients: map[chan Event]struct{}{}}
	if len(cfg.EncryptionKey) == rage.KeyLen {
		var k [rage.KeyLen]byte
		copy(k[:], cfg.EncryptionKey)
		s.encKey = &k
	}
	return s
}

// decryptIfNeeded turns ciphertext on disk back into plaintext for the API.
// Files without a rage magic pass through untouched.
func (s *Server) decryptIfNeeded(data []byte) []byte {
	if s.encKey == nil {
		return data
	}
	// Accept both server-side and password envelope formats — a key can
	// decrypt only the server format, but we still detect the password
	// format so we don't corrupt it by treating it as plaintext.
	if rage.IsServerEncrypted(data) {
		plain, err := rage.DecryptServer(s.encKey[:], data)
		if err != nil {
			return data
		}
		return plain
	}
	return data
}

// encryptIfNeeded encrypts a plaintext before writing to disk.
// Files already carrying a rage magic are left alone.
func (s *Server) encryptIfNeeded(data []byte) []byte {
	if s.encKey == nil || rage.IsServerEncrypted(data) || rage.IsRage(data) {
		return data
	}
	ct, err := rage.EncryptServer(s.encKey[:], data)
	if err != nil {
		return data
	}
	return ct
}

// Handler returns the HTTP routes.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/api/docs", s.guard(s.handleList))
	mux.HandleFunc("/api/doc", s.guard(s.handleDoc))
	mux.HandleFunc("/api/history", s.guard(s.handleHistory))
	mux.HandleFunc("/api/diff", s.guard(s.handleDiff))
	mux.HandleFunc("/api/restore", s.guard(s.handleRestore))
	mux.HandleFunc("/api/events", s.guard(s.handleEvents))
	mux.Handle("/", s.static())
	return withLogging(mux)
}

// guard enforces the shared token on every API call.
func (s *Server) guard(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if !s.authorized(r) {
			writeError(w, http.StatusUnauthorized, "需要访问令牌")
			return
		}
		next(w, r)
	}
}

func (s *Server) authorized(r *http.Request) bool {
	if s.cfg.Token == "" {
		return true
	}
	got := ""
	if h := r.Header.Get("Authorization"); strings.HasPrefix(h, "Bearer ") {
		got = strings.TrimSpace(strings.TrimPrefix(h, "Bearer "))
	}
	if got == "" {
		got = r.URL.Query().Get("token")
	}
	if got == "" {
		if c, err := r.Cookie("mdx-token"); err == nil {
			got = c.Value
		}
	}
	return subtle.ConstantTimeCompare([]byte(got), []byte(s.cfg.Token)) == 1
}

func (s *Server) handleList(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "只支持 GET")
		return
	}
	docs, err := List(s.cfg.Root)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, map[string]any{"docs": docs})
}

func (s *Server) handleDoc(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		path := r.URL.Query().Get("path")
		if _, err := CleanPath(path); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		doc, err := Read(s.cfg.Root, path)
		if err != nil {
			writeError(w, http.StatusNotFound, err.Error())
			return
		}
		doc.Content = string(s.decryptIfNeeded([]byte(doc.Content)))
		doc.Base = Hash(doc.Content)
		writeJSON(w, doc)
	case http.MethodPut, http.MethodPost:
		var body struct {
			Path    string `json:"path"`
			Content string `json:"content"`
			Base    string `json:"base"`
		}
		if err := readJSON(r, &body); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		path, err := CleanPath(body.Path)
		if err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		// Conflict check against the on-disk content (decrypted back to plaintext).
		curPlain := ""
		fullPath := filepath.Join(s.cfg.Root, filepath.FromSlash(path))
		if data, rerr := os.ReadFile(fullPath); rerr == nil {
			curPlain = string(s.decryptIfNeeded(data))
		} else if body.Base != "" && !os.IsNotExist(rerr) {
			writeError(w, http.StatusBadRequest, rerr.Error())
			return
		}
		if body.Base != "" && body.Base != Hash(curPlain) {
			writeConflict(w, Doc{Path: path, Content: curPlain, Base: Hash(curPlain)})
			return
		}
		ciphertext := s.encryptIfNeeded([]byte(body.Content))
		if err := WriteFile(s.cfg.Root, path, string(ciphertext)); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		msg := s.cfg.MessagePrefix + " " + path
		if s.encKey != nil && len(ciphertext) != len(body.Content) {
			msg = fmt.Sprintf("%s %s (密文 %d → %d)", s.cfg.MessagePrefix, path, len(body.Content), len(ciphertext))
		}
		if s.cfg.MessagePrefix != "" {
			_, _ = Commit(s.cfg.Root, path, msg)
		}
		saved := Doc{Path: path, Content: body.Content, Base: Hash(body.Content)}
		s.broadcast(Event{Path: saved.Path, Base: saved.Base, Action: "saved", Content: saved.Content})
		writeJSON(w, saved)
	default:
		writeError(w, http.StatusMethodNotAllowed, "只支持 GET 与 PUT")
	}
}

func (s *Server) handleHistory(w http.ResponseWriter, r *http.Request) {
	path := r.URL.Query().Get("path")
	if _, err := CleanPath(path); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	list, err := History(s.cfg.Root, path, 50)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, map[string]any{"commits": list})
}

func (s *Server) handleDiff(w http.ResponseWriter, r *http.Request) {
	path := r.URL.Query().Get("path")
	commit := r.URL.Query().Get("commit")
	if _, err := CleanPath(path); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	diff, err := Diff(s.cfg.Root, path, commit)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, map[string]any{"diff": diff})
}

func (s *Server) handleRestore(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "只支持 POST")
		return
	}
	var body struct {
		Path   string `json:"path"`
		Commit string `json:"commit"`
	}
	if err := readJSON(r, &body); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	path, err := CleanPath(body.Path)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if err := Restore(s.cfg.Root, path, body.Commit); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	// Restore writes raw bytes from git back to disk — re-encrypt if the
	// server holds a key so clients never see unencrypted bytes.
	if s.encKey != nil {
		fullPath := filepath.Join(s.cfg.Root, filepath.FromSlash(path))
		if data, rerr := os.ReadFile(fullPath); rerr == nil {
			plain := s.decryptIfNeeded(data)
			ct := s.encryptIfNeeded(plain)
			if err := os.WriteFile(fullPath, ct, 0o644); err != nil {
				writeError(w, http.StatusInternalServerError, err.Error())
				return
			}
		}
	}
	doc, err := Read(s.cfg.Root, path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	doc.Content = string(s.decryptIfNeeded([]byte(doc.Content)))
	doc.Base = Hash(doc.Content)
	s.broadcast(Event{Path: doc.Path, Base: doc.Base, Action: "restored", Content: doc.Content})
	writeJSON(w, doc)
}

// handleEvents streams change notifications as server-sent events.
func (s *Server) handleEvents(w http.ResponseWriter, r *http.Request) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeError(w, http.StatusInternalServerError, "不支持流式响应")
		return
	}
	ch := make(chan Event, 16)
	s.mu.Lock()
	s.clients[ch] = struct{}{}
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.clients, ch)
		s.mu.Unlock()
		close(ch)
	}()

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	fmt.Fprint(w, "event: ready\ndata: {}\n\n")
	flusher.Flush()

	ping := time.NewTicker(25 * time.Second)
	defer ping.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case <-ping.C:
			fmt.Fprint(w, ": ping\n\n")
			flusher.Flush()
		case ev, ok := <-ch:
			if !ok {
				return
			}
			data, _ := json.Marshal(ev)
			fmt.Fprintf(w, "event: change\ndata: %s\n\n", data)
			flusher.Flush()
		}
	}
}

func (s *Server) broadcast(ev Event) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for ch := range s.clients {
		select {
		case ch <- ev:
		default:
			// A slow reader misses this change; it refetches on focus.
		}
	}
}

func (s *Server) static() http.Handler {
	sub, err := fs.Sub(webFS, "web")
	if err != nil {
		return http.NotFoundHandler()
	}
	files := http.FileServer(http.FS(sub))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// The page needs the token to call the API; hand it over as a cookie
		// when it arrives in the URL, then drop it from the address bar.
		if t := r.URL.Query().Get("token"); t != "" {
			http.SetCookie(w, &http.Cookie{
				Name: "mdx-token", Value: t, Path: "/", SameSite: http.SameSiteStrictMode,
			})
			http.Redirect(w, r, "/", http.StatusSeeOther)
			return
		}
		files.ServeHTTP(w, r)
	})
}

func readJSON(r *http.Request, v any) error {
	defer r.Body.Close()
	data, err := io.ReadAll(io.LimitReader(r.Body, 32<<20))
	if err != nil {
		return err
	}
	return json.Unmarshal(data, v)
}

func writeJSON(w http.ResponseWriter, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": msg})
}

func writeConflict(w http.ResponseWriter, current Doc) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusConflict)
	_ = json.NewEncoder(w).Encode(map[string]any{
		"error":    "文档已被其他人修改",
		"conflict": current,
	})
}

func withLogging(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		if strings.HasPrefix(r.URL.Path, "/api/") {
			log.Printf("%s %s %s", r.Method, r.URL.Path, time.Since(start).Round(time.Millisecond))
		}
	})
}
