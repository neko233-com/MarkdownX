package app

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

// recentLimit caps the 近期浏览 list.
const recentLimit = 20

var workspaceDocumentExtensions = [...]string{".md", ".markdown", ".text", ".txt"}

type workspaceLaunch uint8

const (
	workspaceLaunchNew workspaceLaunch = iota + 1
	workspaceLaunchFolder
	workspaceLaunchDocumentAndFolder
)

type workspaceOpenMode uint8

const (
	workspaceOpenCurrent workspaceOpenMode = iota
	workspaceOpenTab
	workspaceOpenWindow
)

// settings is what the app remembers between runs.
type settings struct {
	Root string `json:"root,omitempty"`
	Mode Mode   `json:"mode,omitempty"`
	// Last is the document opened last, restored when no path is given.
	Last string `json:"last,omitempty"`
	// Recent is the 近期浏览 list, most recent first.
	Recent           []string          `json:"recent,omitempty"`
	HideSidebar      bool              `json:"hide_sidebar,omitempty"`
	SidebarPage      sidebarPage       `json:"sidebar_page,omitempty"`
	SidebarWidth     float32           `json:"sidebar_width,omitempty"`
	FocusMode        bool              `json:"focus_mode,omitempty"`
	Typewriter       bool              `json:"typewriter,omitempty"`
	AutoSave         bool              `json:"auto_save,omitempty"`
	Diagrams         bool              `json:"diagrams,omitempty"`
	PDF              *pdfPreferences   `json:"pdf,omitempty"`
	LastExport       string            `json:"last_export,omitempty"`
	TreeOrder        workspaceOrder    `json:"tree_order,omitzero"`
	TreeFilter       workspaceFilter   `json:"tree_filter,omitzero"`
	TreeDocumentExt  string            `json:"tree_document_extension,omitempty"`
	TreeOpenMode     workspaceOpenMode `json:"tree_open_mode,omitempty"`
	PinnedFolders    []string          `json:"pinned_folders,omitempty"`
	RecentFolders    []string          `json:"recent_folders,omitempty"`
	OnLaunch         workspaceLaunch   `json:"workspace_on_launch,omitempty"`
	DefaultWorkspace string            `json:"default_workspace,omitempty"`
}

// settingsPath is the per-user configuration file.
func settingsPath() string {
	dir, err := os.UserConfigDir()
	if err != nil || dir == "" {
		return ""
	}
	return filepath.Join(dir, "MarkdownX", "settings.json")
}

// loadSettings reads the saved settings and fills in new preferences.
func loadSettings() settings {
	var s settings
	if p := settingsPath(); p != "" {
		if data, err := os.ReadFile(p); err == nil {
			_ = json.Unmarshal(data, &s)
		}
	}
	if s.OnLaunch == 0 && (s.Root != "" || s.Last != "") {
		// Existing profiles remembered the document and workspace by default.
		s.OnLaunch = workspaceLaunchDocumentAndFolder
	}
	if s.OnLaunch < workspaceLaunchNew || s.OnLaunch > workspaceLaunchDocumentAndFolder {
		s.OnLaunch = workspaceLaunchNew
	}
	if s.SidebarWidth <= 0 {
		s.SidebarWidth = defaultSidebarWidth
	}
	if s.SidebarPage > sidebarOutline {
		s.SidebarPage = sidebarFiles
	}
	if !slices.Contains(workspaceDocumentExtensions[:], s.TreeDocumentExt) {
		s.TreeDocumentExt = workspaceDocumentExtensions[0]
	}
	if s.TreeOpenMode > workspaceOpenWindow {
		s.TreeOpenMode = workspaceOpenCurrent
	}
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
	a.persistWindow(false)
}

func (a *App) persistWindow(openedFolder bool) {
	s := loadSettings()
	if a.root != "" {
		if openedFolder || !sameWorkspacePath(s.Root, a.root) {
			s.RecentFolders = recentWorkspaceFolders(s.RecentFolders, a.root)
		}
		s.Root = a.root
	}
	if a.path != "" {
		s.Last = a.path
		s.Recent = pushRecent(s.Recent, a.path)
		a.recents = s.Recent
	}
	s.Mode = a.mode
	s.HideSidebar = a.hideSide
	s.SidebarPage = a.sidebarPage
	s.FocusMode = a.focusMode
	s.Typewriter = a.typeMode
	s.AutoSave = a.autoSaveOn
	s.Diagrams = a.diagrams
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

// restoreWorkspace applies the startup preferences only on a launch without a path.
func (a *App) restoreWorkspace() {
	s := loadSettings()
	root := s.Root
	if s.OnLaunch == workspaceLaunchNew {
		root = s.DefaultWorkspace
	}
	if path, ok := workspaceFolderPath(root); ok {
		a.root = path
		a.refreshFiles()
		a.resetPreview()
	}
	if s.OnLaunch == workspaceLaunchDocumentAndFolder && s.Last != "" {
		if info, err := os.Stat(s.Last); err == nil && info.Mode().IsRegular() {
			a.openDocument(s.Last, false)
		}
	}
}
