package app

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
)

// recentLimit caps the 近期浏览 list.
const recentLimit = 20

// settings is what the app remembers between runs.
type settings struct {
	Root string `json:"root,omitempty"`
	Mode Mode   `json:"mode,omitempty"`
	// Last is the document opened last, restored when no path is given.
	Last string `json:"last,omitempty"`
	// Recent is the 近期浏览 list, most recent first.
	Recent []string `json:"recent,omitempty"`
	// Tab is the sidebar tab the user left open.
	Tab int `json:"tab,omitempty"`
}

// settingsPath is the per-user configuration file.
func settingsPath() string {
	dir, err := os.UserConfigDir()
	if err != nil || dir == "" {
		return ""
	}
	return filepath.Join(dir, "MarkdownX", "settings.json")
}

// loadSettings reads the saved settings, or returns zero settings.
func loadSettings() settings {
	var s settings
	p := settingsPath()
	if p == "" {
		return s
	}
	data, err := os.ReadFile(p)
	if err != nil {
		return s
	}
	_ = json.Unmarshal(data, &s)
	return s
}

// saveSettings writes the settings, ignoring a failure: they are a
// convenience, never a reason to fail an edit.
func saveSettings(s settings) {
	p := settingsPath()
	if p == "" {
		return
	}
	if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
		return
	}
	data, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return
	}
	_ = os.WriteFile(p, data, 0o644)
}

// remember persists the current workspace and document.
func (a *App) remember() {
	s := loadSettings()
	if a.root != "" {
		s.Root = a.root
	}
	if a.path != "" {
		s.Last = a.path
		s.Recent = pushRecent(s.Recent, a.path)
		a.recents = s.Recent
	}
	s.Mode = a.mode
	s.Tab = a.sideTab
	saveSettings(s)
}

// pushRecent puts p first, drops a duplicate and trims the tail.
func pushRecent(list []string, p string) []string {
	out := make([]string, 0, len(list)+1)
	out = append(out, p)
	for _, v := range list {
		if len(out) >= recentLimit {
			break
		}
		if !samePath(v, p) {
			out = append(out, v)
		}
	}
	return out
}

// samePath compares two paths the way this platform's file system does.
func samePath(a, b string) bool {
	if a == b {
		return true
	}
	return strings.EqualFold(filepath.Clean(a), filepath.Clean(b))
}

// restore reopens the last workspace, and the last document when no path was
// given on the command line.
func (a *App) restore(path string) {
	s := loadSettings()
	a.mode = s.Mode
	a.recents = s.Recent
	a.sideTab = s.Tab
	if path == "" && s.Last != "" {
		if _, err := os.Stat(s.Last); err == nil {
			a.OpenFile(s.Last)
			return
		}
	}
	root := s.Root
	if path != "" {
		return
	}
	if root == "" {
		return
	}
	if info, err := os.Stat(root); err != nil || !info.IsDir() {
		return
	}
	a.root = root
	a.refreshFiles()
}
