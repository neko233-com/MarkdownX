package server

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// DocInfo is one document in the workspace listing.
type DocInfo struct {
	Path     string `json:"path"`
	Title    string `json:"title"`
	Modified string `json:"modified"`
	Size     int64  `json:"size"`
}

// Doc is a document with the version the client last saw.
type Doc struct {
	Path    string `json:"path"`
	Content string `json:"content"`
	// Base is the content hash: send it back to detect a concurrent edit.
	Base     string `json:"base"`
	Modified string `json:"modified"`
}

// ErrNotFound is returned for a path outside the workspace or a missing file.
var ErrNotFound = fmt.Errorf("找不到文档")

// ErrConflict is returned when the client's base is stale.
type ErrConflict struct {
	// Current is the document as it stands now.
	Current Doc
}

func (e *ErrConflict) Error() string { return "文档已被其他人修改" }

// Hash is the version stamp of a document's content.
func Hash(content string) string {
	sum := sha256.Sum256([]byte(content))
	return hex.EncodeToString(sum[:])
}

// CleanPath validates a workspace-relative document path.
func CleanPath(rel string) (string, error) {
	rel = strings.TrimSpace(strings.ReplaceAll(rel, "\\", "/"))
	rel = strings.TrimPrefix(rel, "./")
	if rel == "" || len(rel) > 8192 || strings.HasPrefix(rel, "/") {
		return "", ErrNotFound
	}
	if strings.ContainsAny(rel, ":\x00") {
		return "", ErrNotFound
	}
	for _, part := range strings.Split(rel, "/") {
		if part == "" || part == "." || part == ".." {
			return "", ErrNotFound
		}
	}
	switch strings.ToLower(filepath.Ext(rel)) {
	case ".md", ".markdown", ".mdxb":
	default:
		return "", ErrNotFound
	}
	return rel, nil
}

// List walks the workspace for documents.
func List(root string) ([]DocInfo, error) {
	out := []DocInfo{}
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		name := d.Name()
		if d.IsDir() {
			if path != root && (strings.HasPrefix(name, ".") || name == "node_modules" || name == "dist") {
				return filepath.SkipDir
			}
			return nil
		}
		switch strings.ToLower(filepath.Ext(name)) {
		case ".md", ".markdown":
		default:
			return nil
		}
		info, ierr := d.Info()
		if ierr != nil {
			return nil
		}
		rel, rerr := filepath.Rel(root, path)
		if rerr != nil {
			return nil
		}
		rel = filepath.ToSlash(rel)
		out = append(out, DocInfo{
			Path:     rel,
			Title:    titleOf(path),
			Modified: info.ModTime().UTC().Format(time.RFC3339),
			Size:     info.Size(),
		})
		return nil
	})
	sort.Slice(out, func(i, j int) bool { return out[i].Path < out[j].Path })
	return out, err
}

// titleOf reads the first heading of a document, else its file name.
func titleOf(path string) string {
	data, err := os.ReadFile(path)
	if err != nil {
		return strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
	}
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "#") {
			t := strings.TrimSpace(strings.TrimLeft(line, "#"))
			if t != "" {
				return t
			}
		}
	}
	return strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
}

// Read loads a document from the workspace.
func Read(root, rel string) (Doc, error) {
	clean, err := CleanPath(rel)
	if err != nil {
		return Doc{}, err
	}
	data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(clean)))
	if err != nil {
		return Doc{}, ErrNotFound
	}
	info, _ := os.Stat(filepath.Join(root, filepath.FromSlash(clean)))
	modified := ""
	if info != nil {
		modified = info.ModTime().UTC().Format(time.RFC3339)
	}
	return Doc{Path: clean, Content: string(data), Base: Hash(string(data)), Modified: modified}, nil
}

// Write saves a document, refusing a write based on a stale version.
func Write(root, rel, content, base string) (Doc, error) {
	clean, err := CleanPath(rel)
	if err != nil {
		return Doc{}, err
	}
	full := filepath.Join(root, filepath.FromSlash(clean))
	if current, rerr := os.ReadFile(full); rerr == nil {
		cur := Hash(string(current))
		if base != "" && base != cur {
			return Doc{}, &ErrConflict{Current: Doc{
				Path: clean, Content: string(current), Base: cur,
			}}
		}
	} else if base != "" && !os.IsNotExist(rerr) {
		return Doc{}, rerr
	}
	if err := WriteFile(root, clean, content); err != nil {
		return Doc{}, err
	}
	return Read(root, clean)
}

// WriteFile is Write without the version check, for restores.
func WriteFile(root, rel, content string) error {
	clean, err := CleanPath(rel)
	if err != nil {
		return err
	}
	full := filepath.Join(root, filepath.FromSlash(clean))
	if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
		return err
	}
	return os.WriteFile(full, []byte(content), 0o644)
}
