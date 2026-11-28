package app

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/neko233/markdownx/internal/doc"
)

const utf8BOM = "\ufeff"

type markdownFile struct {
	path     string
	disk     []byte
	text     string
	eol      string
	bom      bool
	identity os.FileInfo // Captured from the handle that read or wrote disk.
}

type autoSave struct {
	path     string
	gen      uint64
	source   *sourceDocument
	revision uint64
	due      time.Time
	tried    bool
}

func (a *App) tickAutoSave(c *ui.Context) {
	active := a.documentTab
	for tab := range a.openDocuments {
		if tab.auto.ready(c, tab, a.autoSaveOn) {
			a.withDocumentPane(tab, func() { a.writeDocumentTab(tab.path, tab == active) })
		}
	}
}

func (a *App) saveAllDocuments() {
	windows := []*App{a}
	if a.windows != nil {
		windows = slices.Clone(a.windows.order)
	}
	seen := make(map[*documentState]bool)
	for _, window := range windows {
		for _, tab := range window.tabs {
			for _, pane := range tab.panes() {
				if seen[pane.documentState] {
					continue
				}
				seen[pane.documentState] = true
				window.withDocumentPane(pane, func() {
					window.syncAll()
					window.updateDirty()
					if pane.dirty {
						window.Save()
					}
				})
			}
		}
		window.services.Invalidate()
	}
}

func (a *App) duplicateDocument() {
	a.syncAll()
	text := a.markdownSource()
	tab := newDocumentTab(doc.ReadSource(text))
	if text != "" {
		tab.savedText = ""
		tab.savedGeneration = ^uint64(0)
		tab.dirty = true
	}
	a.insertDocumentTab(tab, a.tabIndex()+1, true)
}

func rememberExport(format string) {
	settings := loadSettings()
	settings.LastExport = format
	saveSettings(settings)
}

func (a *App) exportPrevious() {
	switch loadSettings().LastExport {
	case "pdf":
		a.ExportPDF()
	case "mdxb":
		a.ExportBook()
	default:
		a.ExportHTML()
	}
}

func (s *autoSave) ready(c *ui.Context, tab *documentTab, enabled bool) bool {
	if !enabled || !tab.dirty || tab.file.path == "" || tab.path != tab.file.path {
		s.due = time.Time{}
		return false
	}
	var revision uint64
	if tab.sourceDocument != nil {
		revision = tab.sourceDocument.buffer.Version()
	}
	if s.due.IsZero() || s.path != tab.path || s.gen != tab.core.Generation || s.source != tab.sourceDocument || s.revision != revision {
		s.path, s.gen, s.source, s.revision = tab.path, tab.core.Generation, tab.sourceDocument, revision
		s.due = c.Now().Add(1200 * time.Millisecond)
		s.tried = false
	}
	if s.tried {
		return false
	}
	if wait := s.due.Sub(c.Now()); wait > 0 {
		c.After(wait)
		return false
	}
	s.tried = true
	return true
}

func readEditorFile(path string) ([]byte, os.FileInfo, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, nil, err
	}
	defer f.Close()
	info, _ := f.Stat()
	data, err := io.ReadAll(f)
	return data, info, err
}

func readMarkdownFile(path string, data []byte) markdownFile {
	raw := string(data)
	crlf := strings.Count(raw, "\r\n")
	cr := strings.Count(raw, "\r") - crlf
	lf := strings.Count(raw, "\n") - crlf
	eol := "\n"
	if crlf > 0 && crlf >= lf && crlf >= cr {
		eol = "\r\n"
	} else if cr > lf {
		eol = "\r"
	}
	text, bom := strings.CutPrefix(raw, utf8BOM)
	text = strings.NewReplacer("\r\n", "\n", "\r", "\n").Replace(text)
	return markdownFile{path: path, disk: data, text: text, eol: eol, bom: bom}
}

func (f *markdownFile) save(path, source string) error {
	same := f.path != "" && samePath(path, f.path)
	if same && source == f.text {
		return nil
	}
	target := path
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		target = resolved
	}
	out := source
	if same {
		out = strings.ReplaceAll(out, "\n", f.eol)
		if f.bom {
			out = utf8BOM + out
		}
	}
	data := []byte(out)
	info, err := replaceMarkdownFile(target, data, f.disk, same)
	if err != nil {
		return err
	}
	*f = readMarkdownFile(path, data)
	f.identity = info
	return nil
}

func writeMarkdown(path string, data, lastRead []byte, check bool) error {
	_, err := replaceMarkdownFile(path, data, lastRead, check)
	return err
}

func replaceMarkdownFile(path string, data, lastRead []byte, check bool) (os.FileInfo, error) {
	tmp, err := os.CreateTemp(filepath.Dir(path), ".markdownx-*.tmp")
	if err != nil {
		return nil, err
	}
	defer os.Remove(tmp.Name())
	defer tmp.Close()
	if st, err := os.Stat(path); err == nil {
		if err := tmp.Chmod(st.Mode().Perm()); err != nil {
			return nil, err
		}
	}
	if _, err := tmp.Write(data); err != nil {
		return nil, err
	}
	if err := tmp.Sync(); err != nil {
		return nil, err
	}
	info, _ := tmp.Stat()
	if err := tmp.Close(); err != nil {
		return nil, err
	}
	if check {
		current, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(current, lastRead) {
			return nil, fmt.Errorf("磁盘上的文件已被修改或移走，编辑内容仍在窗口中")
		}
	}
	if err := os.Rename(tmp.Name(), path); err != nil {
		return nil, err
	}
	return info, nil
}
