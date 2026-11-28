// Package app is MarkdownX's desktop shell, drawn with MyGo's native UI.
package app

import (
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"

	"github.com/neko233/markdownx/internal/book"
	"github.com/neko233/markdownx/internal/convert"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
	htmlexport "github.com/neko233/markdownx/internal/export"
	"github.com/neko233/markdownx/internal/rage"
)

// Mode is what the window shows beside the file tree.
type Mode int

const (
	// ModeEdit shows the block editor only.
	ModeEdit Mode = iota
	// ModeSplit shows the editor and the rendered document.
	ModeSplit
	// ModePreview shows the rendered document only.
	ModePreview
)

// documentState is shared by every pane editing the same open document.
type documentState struct {
	core  *editor.Core
	title *string
	path  string
	dirty bool
	file  markdownFile
	auto  autoSave

	savedGeneration uint64
	savedText       string
	sourceDocument  *sourceDocument
	sourceDoc       *doc.Document
	sourceGen       uint64
	sourceText      string
	sourceRanges    []doc.SourceRange
	statGen         uint64
	statText        string
	selectionOwner  *documentTab
	published       documentRevision
}

// documentTab holds one editor's viewport and input state.
// A split arranges several such panes under one tab-strip entry.
type documentTab struct {
	*documentState
	split         *documentSplit
	splitWeight   float32
	selection     editor.Selection
	pendingMarks  *doc.Marks
	selectionCore *editor.Core
	selectionGen  uint64
	selectionText [2]string
	bufs          map[doc.BlockID]*blockBuffer
	source        *sourceView
	previewScroll ui.ScrollState
	focus         doc.BlockID
	status        string

	// sel is each block's last known selection, as byte offsets.
	sel map[doc.BlockID][2]int
	// pending restores a selection after an edit rebuilt the block's text,
	// as rune offsets for the text input.
	pending                  map[doc.BlockID][2]int
	rangeCore                *editor.Core
	rangeGen                 uint64
	rangeSel                 editor.Selection
	rangeSpans               map[doc.BlockID]editor.Span
	drag                     *selectionDrag
	inlineEdit               *inlineEditor
	inlineRequest            *inlineSourceRequest
	imageEdit                *imageSource
	inlinePause, inlineClick editor.Pos
	pendingLink              string
	pendingNote              editor.Pos
	noteJump                 footnoteJump
	revealTitle              bool
	keepViewportFocus        bool
	referenceIndex           referenceIndex
	editScroll               ui.ScrollState
	editBounds               ui.Rect

	md            markdown.Options
	imageMD       markdown.Options
	previewFolder string

	// find bar state
	finding     bool
	findFocus   bool
	findQuery   string
	findReplace string
	findKey     string
	replacing   bool
	matches     []findHit
	matchIndex  int
	// focusReq asks the next frame to focus a block's text input.
	focusReq doc.BlockID

	reveal                             doc.BlockID
	lastType                           editor.Pos
	templateSession                    *workspaceTemplateSession
	tableRow                           *tableRowSelection
	diagramClosed, diagramPreviewFocus doc.BlockID
}

// App is a desktop window with one workspace and one or more document tabs.
type App struct {
	*documentTab
	tabs        []*documentTab
	closedTabs  []closedDocumentTab
	tabReveal   bool
	tabScroll   ui.ScrollState
	pendingPane *documentTab
	root        string
	files       []string
	mode        Mode
	autoSaveOn  bool
	win         *mygo.Window
	lastTitle   string

	// encDialog drives the password EncryptSave modal.
	encOpen bool
	encPw1  string
	encPw2  string

	// sidebar state
	recents                    []string
	templates                  []template
	hideSide                   bool
	sidebarPage                sidebarPage
	sidebarWidth               float32
	sidebarDrag                *float32
	outline                    documentOutline
	zenMode                    bool
	palette                    commandPalette
	templateLibraryOpen        bool
	focusMode                  bool
	typeMode                   bool
	inlineColor                ui.Color
	inlineAccent               ui.Color
	tree                       *workspaceNode
	treeNodes                  map[string]*workspaceNode
	treeView                   *workspaceTreeView
	treeFocus                  bool
	treeTarget                 string
	treeReveal                 string
	treeCheckAt                time.Time
	treeWatch                  *workspaceWatch
	treeOrder                  workspaceOrder
	treeFilter                 workspaceFilter
	treeFilterEdit             workspaceFilterDialog
	treePreview                workspacePreview
	treeSearch                 workspaceSearchState
	treeRevision               uint64
	treeDropTarget             string
	treeDropExpand             time.Time
	treeDropCopy, treeDropSeen bool
	treePointerHeld            bool
	treeEdit                   workspaceDialog
	treeUndo                   workspaceUndo
	quickOpen                  bool
	quickRecent                bool
	quickFocus                 bool
	quickActions               bool
	quickQuery                 string
	quickIndex                 int
	quickCategory              quickCategory
	services                   ui.Services
	zoom                       float32
	prefsOpen                  bool
	pdfDialog                  pdfDialog
	pdfJob                     chan pdfResult
	tableSize                  tableSizeDialog
	windows                    *desktopWindows
	equation                   equationDialog
	mathCache                  map[mathKey]mathImage
	diagrams                   bool
	diagramCache               map[diagramKey]diagramImage

	// tmplDialog names a template when saving the document as one.
	tmplOpen bool
	tmplName string
}

// New makes an app, opening path when it names a file.
func New(path string) *App {
	s := loadSettings()
	a := &App{
		mode:         ModeEdit,
		recents:      s.Recent,
		templates:    loadTemplates(),
		hideSide:     s.HideSidebar,
		sidebarPage:  s.SidebarPage,
		sidebarWidth: s.SidebarWidth,
		focusMode:    s.FocusMode,
		typeMode:     s.Typewriter,
		treeOrder:    s.TreeOrder,
		treeFilter:   s.TreeFilter,
		autoSaveOn:   s.AutoSave,
		zoom:         1,
		mathCache:    make(map[mathKey]mathImage),
		diagrams:     s.Diagrams,
	}
	a.documentTab = newDocumentTab(doc.NewDocument())
	a.tabs = []*documentTab{a.documentTab}
	a.resetPreview()
	if path != "" {
		a.OpenFile(path)
	}
	a.ResetBuffers()
	return a
}

// OpenFile loads a supported document or opens a directory as the workspace.
func (a *App) OpenFile(path string) {
	if info, err := os.Stat(path); err == nil && info.IsDir() {
		a.openWorkspaceFolder(path)
		return
	}
	a.openDocument(path, true)
}

func (a *App) openDocument(path string, useParentFolder bool) {
	path = absoluteDocumentPath(path)
	if tab := a.tabAt(path); tab != nil {
		if tab.documentState != a.documentState {
			a.selectDocumentTab(tab, false)
			return
		}
		if a.hasOtherPane(a.documentTab) {
			return
		}
	}
	if !a.hasOtherPane(a.documentTab) && !a.confirmDiscard() {
		return
	}
	tab, err := loadDocumentTab(path)
	if err != nil {
		a.status = "打开失败：" + err.Error()
		return
	}
	a.parkDocumentTab()
	if !sameWorkspacePath(a.path, path) && !a.hasOtherPane(a.documentTab) {
		a.rememberClosedTab(a.documentTab, a.tabIndex())
	}
	a.replaceDocumentPane(tab)
	a.showOpenedDocument(useParentFolder)
}

func (a *App) showOpenedDocument(useParentFolder bool) {
	a.resetPreview()
	if a.root == "" && useParentFolder && a.path != "" {
		a.root = filepath.Dir(a.path)
		a.refreshFiles()
	}
	a.revealWorkspaceFile()
	a.tabReveal = true
	a.remember()
}

// Save writes the document back in the format it came from, asking for a path
// the first time.
func (a *App) Save() {
	if a.path != "" && convert.Detect(a.path) == convert.FormatMdxb {
		a.saveBook()
		return
	}
	if a.path == "" || convert.Detect(a.path) != convert.FormatMarkdown {
		a.SaveAs()
		return
	}
	a.saveMarkdown(a.path)
}

func (a *App) SaveAs() {
	name := strings.TrimSuffix(a.DisplayName(), filepath.Ext(a.DisplayName())) + ".md"
	if a.path != "" {
		name = filepath.Join(filepath.Dir(a.path), name)
	}
	path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent: a.win, Title: "另存为 Markdown", DefaultPath: name,
		Filters: []mygo.FileFilter{{Name: "Markdown", Extensions: []string{"md"}}},
	})
	if err != nil || path == "" {
		return
	}
	if convert.Detect(path) != convert.FormatMarkdown {
		path += ".md"
	}
	a.saveMarkdown(path)
}

func (a *App) saveMarkdown(path string) {
	a.writeDocumentTab(path, true)
}

func (a *App) writeDocumentTab(path string, remember bool) {
	if existing := a.tabAt(absoluteDocumentPath(path)); existing != nil && existing.documentState != a.documentState {
		a.status = "保存失败：该文件已在另一个标签页中打开"
		return
	}
	a.syncAll()
	undos := a.workspaceUndosForSave(path)
	source := a.markdownSource()
	changed := source != a.file.text
	if err := a.file.save(path, source); err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	if len(undos) > 0 {
		if info, err := os.Lstat(path); err == nil {
			for _, undo := range undos {
				if sameWorkspacePath(undo.from, path) {
					undo.info = info
				}
				if changed && undo.images[path].failed {
					delete(undo.images, path)
				}
			}
		}
	}
	if a.path != path {
		a.path = path
		if a.root == "" {
			a.root = filepath.Dir(path)
		}
		a.resetPreview()
		a.refreshFiles()
		a.revealWorkspaceFile()
	}
	a.markSaved()
	a.status = "已保存 " + filepath.Base(a.path)
	if remember {
		a.remember()
	}
}

// saveBook writes the edited document back into the `.mdxb` it came from,
// keeping the book's envelope and matching chapters by title.
func (a *App) saveBook() {
	d := a.currentDocument()
	data, err := os.ReadFile(a.path)
	if err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	b, err := book.Parse(data)
	if err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	b.UpdateFromDocument(d)
	out, err := b.Bytes()
	if err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	if err := os.WriteFile(a.path, out, 0o644); err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	a.markSaved()
	a.status = "已保存书籍 " + filepath.Base(a.path)
	a.remember()
}

// ExportHTML writes a self-contained HTML page next to the document.
func (a *App) ExportHTML() {
	d := a.currentDocument()
	name := a.titleText()
	if name == "" {
		name = "document"
	}
	p, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent:      a.win,
		Title:       "导出 HTML",
		DefaultPath: name + ".html",
		Filters:     []mygo.FileFilter{{Name: "HTML", Extensions: []string{"html"}}},
	})
	if err != nil || p == "" {
		return
	}
	page := htmlexport.Page(d, htmlexport.Options{Title: name, Diagrams: a.diagrams})
	if err := os.WriteFile(p, []byte(page), 0o644); err != nil {
		a.status = "导出失败：" + err.Error()
		return
	}
	a.status = "已导出 " + filepath.Base(p)
	rememberExport("html")
}

// ExportBook writes the document as a `.mdxb` book.
func (a *App) ExportBook() {
	d := a.currentDocument()
	name := a.DisplayName()
	if name == "" {
		name = "book"
	}
	p, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
		Parent:      a.win,
		Title:       "导出书籍",
		DefaultPath: name + ".mdxb",
		Filters:     []mygo.FileFilter{{Name: "MarkdownX 书籍", Extensions: []string{"mdxb"}}},
	})
	if err != nil || p == "" {
		return
	}
	if !strings.HasSuffix(strings.ToLower(p), ".mdxb") {
		p += ".mdxb"
	}
	b := book.FromDocument(d, a.titleText())
	data, err := b.Bytes()
	if err != nil {
		a.status = "导出失败：" + err.Error()
		return
	}
	if err := os.WriteFile(p, data, 0o644); err != nil {
		a.status = "导出失败：" + err.Error()
		return
	}
	a.status = "已导出书籍 " + filepath.Base(p)
	rememberExport("mdxb")
}

// ChooseFile opens a document through the system dialog.
func (a *App) ChooseFile() {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent: a.win,
		Title:  "打开",
		Filters: []mygo.FileFilter{
			{Name: "Markdown", Extensions: []string{"md", "markdown", "txt", "qmd"}},
			{Name: "文档", Extensions: []string{"docx", "xlsx", "epub", "ofd", "pdf", "rtf", "html"}},
			{Name: "MarkdownX 书籍", Extensions: []string{"mdxb"}},
		},
	})
	if err != nil || len(paths) == 0 {
		return
	}
	if a.windows != nil {
		a.windows.openFile(paths[0])
	} else {
		a.OpenFile(paths[0])
	}
}

// ChooseFolder opens a folder as the workspace.
func (a *App) ChooseFolder() {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:    a.win,
		Title:     "打开文件夹",
		Directory: true,
	})
	if err != nil || len(paths) == 0 {
		return
	}
	a.openWorkspaceFolder(paths[0])
}

func (a *App) openWorkspaceFolder(path string) {
	absolute, ok := workspaceFolderPath(path)
	if !ok {
		a.status = "无法打开文件夹 " + filepath.Base(path)
		return
	}
	a.root = absolute
	a.refreshFiles()
	a.focusWorkspaceTree()
	a.resetPreview()
	a.status = "工作区 " + filepath.Base(a.root)
	a.persistWindow(true)
}

func (a *App) closeWorkspaceFolder() {
	a.stopWorkspaceWatch()
	a.root = ""
	a.treeUndo = workspaceUndo{}
	a.refreshFiles()
	a.focusWorkspaceTree()
	a.resetPreview()
	a.status = "已关闭文件夹"
	s := loadSettings()
	s.Root = ""
	saveSettings(s)
}

// SaveAsTemplate opens the naming dialog for "存为模板".
func (a *App) SaveAsTemplate() {
	a.tmplOpen = true
	a.tmplName = a.templateDefaultName()
}

// templateDefaultName names the template after the document.
func (a *App) templateDefaultName() string {
	if a.path != "" {
		return strings.TrimSuffix(filepath.Base(a.path), filepath.Ext(a.path))
	}
	if t := a.titleText(); t != "" {
		return t
	}
	return "未命名模板"
}

// doSaveTemplate writes the open document into the template folder.
func (a *App) doSaveTemplate() {
	a.tmplOpen = false
	if strings.TrimSpace(a.tmplName) == "" {
		a.status = "模板名不能为空"
		return
	}
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
	body := doc.Export(a.core.Doc, doc.Options{}).Text
	p, err := saveTemplate(a.tmplName, body)
	if err != nil {
		a.status = "模板保存失败：" + err.Error()
		return
	}
	a.templates = loadTemplates()
	a.templateLibraryOpen = true
	a.status = "已存为模板 " + strings.TrimSuffix(filepath.Base(p), filepath.Ext(p))
}

// ImportTemplate copies a .md file into the template folder.
func (a *App) ImportTemplate() {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:  a.win,
		Title:   "导入 .md 为模板",
		Filters: []mygo.FileFilter{{Name: "Markdown", Extensions: []string{"md", "markdown", "txt"}}},
	})
	if err != nil || len(paths) == 0 {
		return
	}
	if err := a.ImportTemplateFile(paths[0]); err != nil {
		a.status = "模板导入失败：" + err.Error()
	}
}

// ImportTemplateFile stores a Markdown file's content as a template, named
// after the file. It is what the dialog calls once a file is chosen.
func (a *App) ImportTemplateFile(path string) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	name := strings.TrimSuffix(filepath.Base(path), filepath.Ext(path))
	if _, err := saveTemplate(name, string(data)); err != nil {
		return err
	}
	a.templates = loadTemplates()
	a.templateLibraryOpen = true
	a.status = "已导入模板 " + name
	return nil
}

// DeleteTemplate removes a user template. Built-ins cannot be removed.
func (a *App) DeleteTemplate(t template) {
	if t.Path == "" {
		return
	}
	if err := os.Remove(t.Path); err != nil {
		a.status = "删除模板失败：" + err.Error()
		return
	}
	a.templates = loadTemplates()
	a.status = "已删除模板 " + t.Name
}

// InsertTemplate appends a template's Markdown at the end of the document.
func (a *App) InsertTemplate(t template) {
	blocks := doc.MarkdownToBlocks(t.Body, doc.Off{File: true})
	if len(blocks) == 0 {
		a.status = "模板为空：" + t.Name
		return
	}
	a.leaveSource()
	a.syncAll()
	a.core.InsertBlocks(editor.Title, blocks...)
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
	a.status = "已插入模板 " + t.Name
}

func (a *documentTab) titleText() string { return strings.TrimSpace(*a.title) }

// DisplayName is the document's name in the window title and dialogs.
func (a *documentTab) DisplayName() string {
	if a.path != "" {
		return filepath.Base(a.path)
	}
	if t := a.titleText(); t != "" {
		return t
	}
	return "未命名"
}

// SyncTitle puts the document name and the unsaved marker in the window title.
func (a *App) SyncTitle() {
	if a.win == nil {
		return
	}
	name := a.DisplayName()
	if a.dirty {
		name += " •"
	}
	if name == a.lastTitle {
		return
	}
	a.lastTitle = name
	a.win.SetTitle(name + " — MarkdownX")
}

// Stats is the status bar's character count, recomputed once per edit.
func (a *App) Stats() string {
	if a.statGen == a.core.Generation && a.statText != "" {
		return a.statText
	}
	runes := utf8.RuneCountInString(a.core.Doc.PlainText())
	a.statGen = a.core.Generation
	a.statText = itoa(runes) + " 字"
	return a.statText
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var b [20]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	return string(b[i:])
}

// ResetBuffers refreshes the per-block inputs from the model in place, so the
// text areas keep their identity and their caret across an edit.
func (a *App) ResetBuffers() {
	seen := map[doc.BlockID]bool{}
	a.core.Doc.ForEachBlock(func(b *doc.Block) {
		seen[b.ID] = true
		if p := a.bufs[b.ID]; p != nil {
			a.updateBlockBuffer(b, p)
		}
	})
	for id := range a.bufs {
		if !seen[id] {
			delete(a.bufs, id)
			delete(a.sel, id)
			delete(a.pending, id)
		}
	}
	*a.title = a.core.Doc.Title.Text
	a.updateDirty()
}

func (a *App) resetDocument(d *doc.Document) {
	a.templateSession = nil
	a.core = editor.New(d)
	a.selectionOwner = a.documentTab
	a.clearSharedSource()
	a.source = nil
	a.tableRow = nil
	a.drag = nil
	a.inlineEdit = nil
	a.inlineRequest = nil
	a.imageEdit = nil
	a.noteJump, a.pendingNote = footnoteJump{}, editor.Pos{}
	a.revealTitle = false
	a.inlinePause, a.inlineClick = editor.Pos{}, editor.Pos{}
	a.editScroll.Y = 0
	a.diagramClosed, a.diagramPreviewFocus = 0, 0
	a.focus, a.focusReq, a.reveal = 0, 0, 0
	a.lastType = editor.Pos{}
	a.closeFind()
	a.statText = ""
	a.markSaved()
	a.ResetBuffers()
}

func (a *App) markSaved() {
	a.savedGeneration = a.core.Generation
	a.savedText = a.markdownSource()
	a.dirty = false
}

func (a *App) updateDirty() {
	if a.sourceDocument != nil {
		a.dirty = a.sourceDocument.text() != a.savedText
	} else {
		a.dirty = a.core.Generation != a.savedGeneration
	}
}

// restoreSelection asks the text input of id to put its caret back, once.
func (a *App) restoreSelection(id doc.BlockID, start, end int) {
	a.pending[id] = [2]int{start, end}
}

func (a *App) sync(b *doc.Block) {
	if b == nil {
		return
	}
	if p := a.bufs[b.ID]; p != nil {
		a.syncBlockBuffer(b, p)
	}
}

func (a *App) syncAll() {
	a.finishImageSource(true)
	a.syncTitle()
	a.syncRangeInput()
	a.core.Doc.ForEachBlock(func(b *doc.Block) { a.sync(b) })
}

func (a *App) syncRangeInput() {
	if a.core.Sel.AcrossBlocks() {
		a.sync(a.core.Block(a.focus))
	}
}

func (a *App) syncTitle() {
	if *a.title != a.core.Doc.Title.Text {
		a.core.Sel = editor.Caret(editor.Pos{Block: editor.Title})
		a.core.SyncText(editor.Title, *a.title)
		a.updateDirty()
	}
}

func (a *App) focused() *doc.Block {
	if a.source != nil || a.mode == ModePreview {
		return nil
	}
	if b := a.core.Block(a.focus); b != nil {
		return b
	}
	if len(a.core.Doc.Blocks) > 0 {
		return a.core.Doc.Blocks[0]
	}
	return nil
}

func (a *App) setCoreSelection(b *doc.Block) {
	if a.core.Sel.AcrossBlocks() {
		return
	}
	lo, hi := a.selectionOf(b)
	a.core.Sel = editor.Selection{
		Anchor: editor.Pos{Block: b.ID, Offset: lo},
		Head:   editor.Pos{Block: b.ID, Offset: hi},
	}
}

func (a *App) focusAt(sel editor.Selection) {
	a.closeInlineSource(true)
	a.core.Sel = sel
	p := sel.Head
	b := a.core.Block(p.Block)
	if b == nil || !b.Kind.HasText() {
		return
	}
	start := p.Offset
	if sel.Anchor.Block == p.Block {
		start = sel.Anchor.Offset
	}
	a.focus, a.focusReq, a.reveal = p.Block, p.Block, p.Block
	if sel.AcrossBlocks() {
		a.projectBlockSelection(b, a.bufs[b.ID])
		return
	}
	a.restoreSelection(p.Block, doc.ByteToRune(b.Text.Text, start), doc.ByteToRune(b.Text.Text, p.Offset))
}

// setMode switches the pane layout and remembers it.
func (a *App) setMode(m Mode) {
	a.endWorkspaceTemplateSession()
	a.leaveSource()
	a.mode = m
	if m == ModePreview {
		a.drag = nil
	}
	a.remember()
}

func (a *App) bullet() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.ToggleList()
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) ordered() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.ToggleOrdered()
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) todo() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.ToggleTodo()
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) quote() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.ToggleQuote()
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) code() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.ToggleCode()
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) divider() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.insertBlock(doc.NewBlock(doc.DividerKind{}, doc.RichText{}))
}

// insertTable adds a 3×3 table after the focused block.
func (a *App) insertSizedTable(rows, cols int) {
	if a.focused() == nil {
		return
	}
	a.syncAll()
	t := &doc.Table{HeaderRows: 1}
	t.ColWidths = make([]float32, cols)
	for i := range t.ColWidths {
		t.ColWidths[i] = doc.TableDefaultWidth
	}
	t.Rows = make([][]doc.TableCell, rows)
	for r := range t.Rows {
		t.Rows[r] = make([]doc.TableCell, cols)
		for c := range t.Rows[r] {
			t.Rows[r][c] = doc.EmptyCell()
		}
	}
	t.Repair()
	a.insertBlock(doc.NewBlock(doc.TableKind{Table: t}, doc.RichText{}))
	a.focusAt(editor.Caret(editor.Pos{Block: t.Rows[0][0].Blocks[0].ID}))
}

// insertImage copies a picture next to the document and inserts it.
func (a *App) insertImage() {
	if a.focused() == nil {
		return
	}
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:  a.win,
		Title:   "插入图片",
		Filters: []mygo.FileFilter{{Name: "图片", MIMETypes: []string{"image/*"}}},
	})
	if err != nil || len(paths) == 0 {
		return
	}
	src := paths[0]
	ref, err := imageReference(a.path, src)
	if err != nil {
		a.status = "插入图片失败：" + err.Error()
		return
	}
	a.syncAll()
	a.insertBlock(doc.NewBlock(doc.ImageKind{SrcKind: "src", Src: ref}, doc.RichText{}))
}

// insertBlock adds a block after the focused one, then a paragraph to keep
// typing in.
func (a *App) insertBlock(b *doc.Block) {
	cur := a.focused()
	if cur == nil {
		return
	}
	a.core.InsertBlocks(cur.ID, b, doc.TextBlock(""))
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

func (a *App) heading(level int) {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.setCoreSelection(b)
	a.core.SetHeading(level)
	a.ResetBuffers()
	a.focusAt(a.core.Sel)
}

// setMark toggles a character format over the focused block's selection, or
// arms it for the next typed text when nothing is selected.
func (a *App) setMark(has func(*doc.Marks) bool, set func(*doc.Marks, bool)) {
	a.editFocused(func(b *doc.Block) bool {
		if _, code := b.Kind.(doc.CodeKind); code && !a.core.Sel.AcrossBlocks() {
			return false
		}
		a.core.ToggleMark(has, set)
		return true
	})
}

// selectionOf is the focused block's selection in byte offsets, falling back
// to a caret at the end of the line.
func (a *App) selectionOf(b *doc.Block) (int, int) {
	text := b.Text.Text
	if b.ID == a.focus {
		if s, ok := a.sel[b.ID]; ok {
			lo, hi := s[0], s[1]
			if lo > hi {
				lo, hi = hi, lo
			}
			lo = clampOffset(text, lo)
			hi = clampOffset(text, hi)
			return lo, hi
		}
	}
	n := len(text)
	return n, n
}

func clampOffset(s string, n int) int {
	if n < 0 {
		return 0
	}
	if n > len(s) {
		return len(s)
	}
	for n > 0 && n < len(s) && !utf8.RuneStart(s[n]) {
		n--
	}
	return n
}

func (a *App) bold() {
	a.setMark(func(m *doc.Marks) bool { return m.Bold }, func(m *doc.Marks, on bool) { m.Bold = on })
}
func (a *App) italic() {
	a.setMark(func(m *doc.Marks) bool { return m.Italic }, func(m *doc.Marks, on bool) { m.Italic = on })
}
func (a *App) underline() {
	a.setMark(func(m *doc.Marks) bool { return m.Underline }, func(m *doc.Marks, on bool) { m.Underline = on })
}
func (a *App) strike() {
	a.setMark(func(m *doc.Marks) bool { return m.Strike }, func(m *doc.Marks, on bool) { m.Strike = on })
}
func (a *App) inlineCode() {
	a.setMark(func(m *doc.Marks) bool { return m.Code }, func(m *doc.Marks, on bool) { m.Code = on })
}

func (a *App) duplicate() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.DuplicateBlocks([]doc.BlockID{b.ID})
	a.ResetBuffers()
}

func (a *App) undo() {
	a.endWorkspaceTemplateSession()
	if a.source != nil {
		return
	}
	a.syncAll()
	if a.core.Undo() {
		a.ResetBuffers()
		a.focusAt(a.core.Sel)
		a.checkSavedText()
	}
}

func (a *App) redo() {
	a.endWorkspaceTemplateSession()
	if a.source != nil {
		return
	}
	a.syncAll()
	if a.core.Redo() {
		a.ResetBuffers()
		a.focusAt(a.core.Sel)
		a.checkSavedText()
	}
}

func (a *App) checkSavedText() {
	if a.markdownSource() == a.savedText {
		a.savedGeneration = a.core.Generation
		a.dirty = false
	}
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if strings.TrimSpace(v) != "" {
			return v
		}
	}
	return ""
}

func deref(s *string, fallback string) string {
	if s != nil && strings.TrimSpace(*s) != "" {
		return *s
	}
	return fallback
}

// NewDocument starts an empty document.
func (a *App) NewDocument() {
	shared := a.hasOtherPane(a.documentTab)
	if !shared && !a.confirmDiscard() {
		return
	}
	a.parkDocumentTab()
	if !shared {
		a.rememberClosedTab(a.documentTab, a.tabIndex())
	}
	a.replaceDocumentPane(newDocumentTab(doc.NewDocument()))
	a.resetPreview()
}

// confirmDiscard asks what to do about unsaved changes before replacing the
// open document. It reports whether the caller may continue.
func (a *App) confirmDiscard() bool {
	a.syncAll()
	a.updateDirty()
	if !a.dirty {
		return true
	}
	return a.confirmUnsaved(a.documentTab)
}

func (a *App) confirmUnsaved(tab *documentTab) bool {
	res, err := mygo.Dialog.Message(mygo.MessageOptions{
		Parent:        a.win,
		Type:          mygo.MessageWarning,
		Title:         "未保存的修改",
		Message:       "要先保存 " + tab.DisplayName() + " 吗？",
		Buttons:       []string{"保存", "不保存", "取消"},
		DefaultButton: 0,
		CancelButton:  2,
	})
	if err != nil {
		return false
	}
	switch res.Button {
	case 0:
		a.selectDocumentTab(tab, false)
		a.Save()
		return !tab.dirty
	case 1:
		return true
	default:
		return false
	}
}

// EncryptSave opens the password modal; on confirm it writes the document
// encrypted with the chosen password.
func (a *App) EncryptSave() {
	a.encOpen = true
	a.encPw1 = ""
	a.encPw2 = ""
	a.status = "输入两次密码以加密保存"
}

// doEncryptSave executes the actual encrypted write. Called from the view
// after the user clicks OK on the modal.
func (a *App) doEncryptSave() {
	a.encOpen = false
	if a.encPw1 == "" {
		a.status = "密码不能为空"
		return
	}
	if a.encPw1 != a.encPw2 {
		a.status = "两次密码不一致"
		return
	}
	a.syncAll()
	src := a.markdownSource()
	enc, err := rage.EncryptPage([]byte(src), a.encPw1)
	if err != nil {
		a.status = "加密失败：" + err.Error()
		return
	}
	target := a.path
	if target == "" {
		p, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
			Parent:      a.win,
			Title:       "加密保存",
			DefaultPath: a.titleText() + ".md",
			Filters:     []mygo.FileFilter{{Name: "Markdown", Extensions: []string{"md"}}},
		})
		if err != nil || p == "" {
			a.status = "已取消"
			return
		}
		target = p
	}
	if err := os.WriteFile(target, enc, 0o644); err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	a.path = target
	a.markSaved()
	a.status = "已加密保存 " + filepath.Base(target)
	a.encPw1 = ""
	a.encPw2 = ""
}
