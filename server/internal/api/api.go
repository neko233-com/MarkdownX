// Package api exposes the document REST surface and the WebSocket upgrade.
// It deliberately keeps transport concerns out of the room: the room owns
// revision checks and Git commits.
package api

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/neko233-com/MarkdownX/server/internal/config"
	"github.com/neko233-com/MarkdownX/server/internal/gitstore"
	"github.com/neko233-com/MarkdownX/server/internal/live"
	"github.com/neko233-com/MarkdownX/server/internal/store"
)

type Server struct {
	cfg   config.Config
	store *store.Store
	repo  *gitstore.Repo
	hub   *live.Hub
}

func New(cfg config.Config, st *store.Store, repo *gitstore.Repo, hub *live.Hub) *Server {
	return &Server{cfg: cfg, store: st, repo: repo, hub: hub}
}

func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/health", s.health)
	mux.Handle("GET /api/docs", s.auth(http.HandlerFunc(s.list)))
	mux.Handle("POST /api/docs", s.auth(http.HandlerFunc(s.create)))
	mux.Handle("GET /api/docs/{id}", s.auth(http.HandlerFunc(s.read)))
	mux.Handle("PUT /api/docs/{id}", s.auth(http.HandlerFunc(s.write)))
	mux.Handle("GET /api/docs/{id}/history", s.auth(http.HandlerFunc(s.history)))
	mux.Handle("GET /api/docs/{id}/diff", s.auth(http.HandlerFunc(s.diff)))
	mux.Handle("GET /api/docs/{id}/revision/{rev}", s.auth(http.HandlerFunc(s.revision)))
	mux.Handle("POST /api/docs/{id}/restore", s.auth(http.HandlerFunc(s.restore)))
	mux.Handle("GET /ws/docs/{id}", s.auth(http.HandlerFunc(s.websocket)))
	return mux
}

func (s *Server) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "service": "markdownx-server", "time": time.Now().UTC()})
}

func (s *Server) auth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if s.cfg.Token == "" {
			if s.cfg.Dev {
				next.ServeHTTP(w, r)
				return
			}
			writeError(w, http.StatusUnauthorized, "server token is not configured")
			return
		}
		token := bearer(r)
		if token == "" {
			token = r.URL.Query().Get("token")
		}
		if token == "" {
			if c, err := r.Cookie("mx_token"); err == nil {
				token = c.Value
			}
		}
		if subtle.ConstantTimeCompare([]byte(token), []byte(s.cfg.Token)) != 1 {
			writeError(w, http.StatusUnauthorized, "unauthorized")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func bearer(r *http.Request) string {
	h := r.Header.Get("Authorization")
	if strings.HasPrefix(strings.ToLower(h), "bearer ") {
		return strings.TrimSpace(h[7:])
	}
	return ""
}

func (s *Server) list(w http.ResponseWriter, r *http.Request) {
	docs, err := s.store.List()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"docs": docs})
}

type createRequest struct {
	ID      string `json:"id"`
	Title   string `json:"title"`
	Content string `json:"content"`
}

func (s *Server) create(w http.ResponseWriter, r *http.Request) {
	var req createRequest
	if err := decode(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if req.Content == "" {
		title := strings.TrimSpace(req.Title)
		if title == "" {
			title = "Untitled"
		}
		req.Content = "# " + title + "\n"
	}
	id := strings.TrimSpace(req.ID)
	if id == "" {
		id = slug(req.Title)
	}
	if !store.ValidID(id) {
		writeError(w, http.StatusBadRequest, "invalid document id")
		return
	}
	if _, err := s.store.Path(id); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if _, err := s.store.Read(id); err == nil {
		writeError(w, http.StatusConflict, "document already exists")
		return
	}
	if err := s.store.Write(id, []byte(req.Content), s.cfg.MaxBytes); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	rel, _ := s.store.Rel(id)
	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	hash, err := s.repo.Commit(ctx, rel, "create: "+id, s.cfg.CommitName)
	cancel()
	if err != nil && !errors.Is(err, gitstore.ErrNoChanges) {
		writeError(w, http.StatusInternalServerError, "git commit: "+err.Error())
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"id": id, "commit": hash})
}

func (s *Server) read(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	doc, err := s.store.Read(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	info, _ := s.store.Stat(id)
	writeJSON(w, http.StatusOK, map[string]any{
		"id": id, "title": store.Title(string(doc), id), "content": string(doc), "doc": info,
	})
}

type writeRequest struct {
	Base    int64  `json:"base"`
	Content string `json:"content"`
}

func (s *Server) write(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	var req writeRequest
	if err := decode(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if _, err := s.store.Stat(id); err != nil {
		writeStoreError(w, err)
		return
	}
	room, err := s.hub.Room(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	out, err := room.Save(ctx, req.Base, req.Content, s.author(r), "http")
	cancel()
	if err != nil {
		writeStoreError(w, err)
		return
	}
	if out.Conflict != nil {
		writeJSON(w, http.StatusConflict, out.Conflict)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"rev": out.Rev, "title": out.Title, "changed": out.Changed, "commit": out.Commit, "warning": out.GitError,
	})
}

func (s *Server) history(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	rel, err := s.store.Rel(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	limit := 50
	if n, err := strconv.Atoi(r.URL.Query().Get("limit")); err == nil && n > 0 && n <= 200 {
		limit = n
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	log, err := s.repo.Log(ctx, rel, limit)
	cancel()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"commits": log})
}

func (s *Server) diff(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	rel, err := s.store.Rel(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	from, to := r.URL.Query().Get("from"), r.URL.Query().Get("to")
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	text, err := s.repo.Diff(ctx, from, to, rel)
	cancel()
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"from": from, "to": to, "diff": text})
}

func (s *Server) revision(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	rel, err := s.store.Rel(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	content, err := s.repo.Show(ctx, r.PathValue("rev"), rel)
	cancel()
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"rev": r.PathValue("rev"), "content": string(content)})
}

type restoreRequest struct {
	Rev string `json:"rev"`
}

func (s *Server) restore(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	rel, err := s.store.Rel(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	var req restoreRequest
	if err := decode(r, &req); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	room, err := s.hub.Room(id)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	if strings.TrimSpace(req.Rev) == "" {
		writeError(w, http.StatusBadRequest, "revision is required")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	content, err := s.repo.Show(ctx, req.Rev, rel)
	cancel()
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	rev, _, _, _, _ := room.Current()
	ctx, cancel = context.WithTimeout(r.Context(), 15*time.Second)
	out, err := room.Save(ctx, rev, string(content), s.author(r), "restore")
	cancel()
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"rev": out.Rev, "commit": out.Commit, "warning": out.GitError})
}

func (s *Server) websocket(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	if _, err := s.store.Path(id); err != nil {
		writeStoreError(w, err)
		return
	}
	s.hub.ServeWS(w, r, id)
}

func (s *Server) author(r *http.Request) string {
	name := strings.TrimSpace(r.URL.Query().Get("name"))
	if name == "" {
		name = s.cfg.CommitName
	}
	if name == "" {
		name = "anonymous"
	}
	return name
}

func decode(r *http.Request, v any) error {
	defer r.Body.Close()
	dec := json.NewDecoder(io.LimitReader(r.Body, 8<<20))
	dec.DisallowUnknownFields()
	return dec.Decode(v)
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, text string) {
	writeJSON(w, status, map[string]any{"error": text})
}

func writeStoreError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, store.ErrBadID):
		writeError(w, http.StatusBadRequest, err.Error())
	case errors.Is(err, store.ErrNoDoc):
		writeError(w, http.StatusNotFound, err.Error())
	case errors.Is(err, store.ErrTooBig), errors.Is(err, live.ErrTooBig):
		writeError(w, http.StatusRequestEntityTooLarge, err.Error())
	case errors.Is(err, live.ErrStaleBase):
		writeError(w, http.StatusConflict, err.Error())
	default:
		writeError(w, http.StatusInternalServerError, err.Error())
	}
}

func slug(s string) string {
	var b strings.Builder
	lastDash := false
	for _, r := range strings.ToLower(strings.TrimSpace(s)) {
		ok := r >= 'a' && r <= 'z' || r >= '0' && r <= '9'
		if ok {
			b.WriteRune(r)
			lastDash = false
		} else if !lastDash && b.Len() > 0 {
			b.WriteByte('-')
			lastDash = true
		}
		if b.Len() >= 64 {
			break
		}
	}
	out := strings.Trim(b.String(), "-")
	if out == "" {
		out = fmt.Sprintf("doc-%d", time.Now().UnixNano())
	}
	return out
}
