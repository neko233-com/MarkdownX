// Package store owns the Markdown files under <root>/docs. It is deliberately
// unaware of HTTP and WebSockets so the same primitives can be tested alone.
package store

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
)

var (
	ErrBadID   = errors.New("invalid document id")
	ErrNoDoc   = errors.New("document not found")
	ErrTooBig  = errors.New("document is too large")
	idPattern  = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,96}$`)
	titleLimit = 160
)

type Store struct {
	root string
	docs string
	mu   sync.Mutex
}

type Doc struct {
	ID       string    `json:"id"`
	Title    string    `json:"title"`
	Path     string    `json:"-"`
	Size     int64     `json:"size"`
	Modified time.Time `json:"modified"`
}

func Open(root string) (*Store, error) {
	docs := filepath.Join(root, "docs")
	if err := os.MkdirAll(docs, 0o755); err != nil {
		return nil, fmt.Errorf("create docs dir: %w", err)
	}
	return &Store{root: root, docs: docs}, nil
}

func (s *Store) Root() string { return s.root }
func (s *Store) DocsDir() string { return s.docs }

func (s *Store) Path(id string) (string, error) {
	if !ValidID(id) {
		return "", ErrBadID
	}
	return filepath.Join(s.docs, id+".md"), nil
}

func (s *Store) Rel(id string) (string, error) {
	if !ValidID(id) {
		return "", ErrBadID
	}
	return filepath.ToSlash(filepath.Join("docs", id+".md")), nil
}

func ValidID(id string) bool {
	return idPattern.MatchString(id) && !strings.Contains(id, "..")
}

func (s *Store) Read(id string) ([]byte, error) {
	path, err := s.Path(id)
	if err != nil {
		return nil, err
	}
	b, err := os.ReadFile(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, ErrNoDoc
	}
	return b, err
}

func (s *Store) Write(id string, content []byte, maxBytes int64) error {
	if int64(len(content)) > maxBytes {
		return ErrTooBig
	}
	path, err := s.Path(id)
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return AtomicWrite(path, content)
}

// Stat returns one document's metadata, including its current title.
func (s *Store) Stat(id string) (Doc, error) {
	path, err := s.Path(id)
	if err != nil {
		return Doc{}, err
	}
	info, err := os.Stat(path)
	if errors.Is(err, fs.ErrNotExist) {
		return Doc{}, ErrNoDoc
	}
	if err != nil {
		return Doc{}, err
	}
	b, err := os.ReadFile(path)
	if err != nil {
		return Doc{}, err
	}
	return Doc{ID: id, Title: Title(string(b), id), Path: path, Size: int64(len(b)), Modified: info.ModTime()}, nil
}

func (s *Store) List() ([]Doc, error) {
	entries, err := os.ReadDir(s.docs)
	if err != nil {
		return nil, err
	}
	out := make([]Doc, 0, len(entries))
	for _, e := range entries {
		if e.IsDir() || !strings.EqualFold(filepath.Ext(e.Name()), ".md") {
			continue
		}
		id := strings.TrimSuffix(e.Name(), filepath.Ext(e.Name()))
		if !ValidID(id) {
			continue
		}
		path := filepath.Join(s.docs, e.Name())
		b, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		info, _ := e.Info()
		doc := Doc{ID: id, Title: Title(string(b), id), Path: path, Size: int64(len(b))}
		if info != nil {
			doc.Modified = info.ModTime()
		}
		out = append(out, doc)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Modified.After(out[j].Modified) })
	return out, nil
}

// Title returns the first level-one heading, falling back to the id.
func Title(content, fallback string) string {
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(strings.TrimPrefix(line, "\ufeff"))
		if strings.HasPrefix(line, "# ") {
			title := strings.TrimSpace(strings.TrimPrefix(line, "# "))
			if title != "" {
				runes := []rune(title)
				if len(runes) > titleLimit {
					title = string(runes[:titleLimit])
				}
				return title
			}
		}
	}
	return fallback
}

// AtomicWrite replaces a file without ever exposing a half-written document.
func AtomicWrite(path string, content []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	suffix := make([]byte, 6)
	if _, err := rand.Read(suffix); err != nil {
		return err
	}
	tmp := filepath.Join(dir, "."+filepath.Base(path)+".markdownx-"+hex.EncodeToString(suffix)+".tmp")
	f, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return err
	}
	cleanup := func() {
		f.Close()
		os.Remove(tmp)
	}
	if _, err := f.Write(content); err != nil {
		cleanup()
		return err
	}
	if err := f.Sync(); err != nil {
		cleanup()
		return err
	}
	if err := f.Close(); err != nil {
		os.Remove(tmp)
		return err
	}
	if err := os.Rename(tmp, path); err != nil {
		os.Remove(tmp)
		return err
	}
	return nil
}
