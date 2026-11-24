// Package app is MarkdownX's desktop shell, drawn with MyGo's native UI.
package app

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
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

// Mode is what the window shows beside the outline.
type Mode int

const (
	// ModeEdit shows the block editor only.
	ModeEdit Mode = iota
	// ModeSplit shows the editor and the rendered document.
	ModeSplit
	// ModePreview shows the rendered document only.
	ModePreview
)

// App is the window's state.
type App struct {
	core  *editor.Core
	bufs  map[doc.BlockID]*string
	title *string
	path  string
	root  string
	files []string
	mode  Mode
	dirty bool

	focus  doc.BlockID
	status string

	// sel is each block's last known selection, as byte offsets.
	sel map[doc.BlockID][2]int
	// pending restores a selection after an edit rebuilt the block's text,
	// as rune offsets for the text input.
	pending map[doc.BlockID][2]int

	md    markdown.Options
	win   *mygo.Window
	winID int

	lastTitle string
	statGen   uint64
	statText  string

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

	// encDialog drives the password EncryptSave modal.
	encOpen bool
	encPw1  string
	encPw2  string

	// sidebar state
	recents   []string
	templates []template
	sideTab   int

	// tmplDialog names a template when saving the document as one.
	tmplOpen bool
	tmplName string

	// linkDialog asks for the URL of a link mark.
	linkOpen  bool
	linkFocus bool
	linkURL   string
}

// New makes an app, opening path when it names a file.
func New(path string) *App {
	s := loadSettings()
	a := &App{
		bufs:      map[doc.BlockID]*string{},
		sel:       map[doc.BlockID][2]int{},
		pending:   map[doc.BlockID][2]int{},
		title:     new(string),
		mode:      ModeSplit,
		recents:   s.Recent,
		templates: loadTemplates(),
		sideTab:   s.Tab,
	}
	a.core = editor.New(doc.NewDocument())
	if path != "" {
		a.open(path)
	}
	a.ResetBuffers()
	return a
}

// OpenFile loads a file of any supported format into the editor.
func (a *App) OpenFile(path string) {
	if !a.confirmDiscard() {
		return
	}
	data, err := os.ReadFile(path)
	if err != nil {
		a.status = "打开失败：" + err.Error()
		return
	}
	text, err := convert.Markdown(path, data)
	if err != nil {
		a.status = err.Error()
		return
	}
	a.core = editor.New(&doc.Document{Blocks: doc.MarkdownToBlocks(text, doc.Off{File: true})})
	if a.core.Doc.Blocks == nil {
		a.core = editor.New(doc.NewDocument())
	}
	a.path = path
	a.dirty = false
	a.status = ""
	if a.root == "" {
		a.root = filepath.Dir(path)
		a.refreshFiles()
	}
	a.ResetBuffers()
	a.remember()
}

func (a *App) open(path string) { a.OpenFile(path) }

// Save writes the document back in the format it came from, asking for a path
// the first time.
func (a *App) Save() {
	if a.path != "" && convert.Detect(a.path) == convert.FormatMdxb {
		a.saveBook()
		return
	}
	if a.path == "" || convert.Detect(a.path) != convert.FormatMarkdown {
		p, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
			Parent:      a.win,
			Title:       "保存",
			DefaultPath: a.titleText() + ".md",
			Filters:     []mygo.FileFilter{{Name: "Markdown", Extensions: []string{"md"}}},
		})
		if err != nil || p == "" {
			return
		}
		if !strings.HasSuffix(strings.ToLower(p), ".md") {
			p += ".md"
		}
		a.path = p
	}
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
	src := doc.Export(a.core.Doc, doc.Options{}).Text
	if err := os.WriteFile(a.path, []byte(src), 0o644); err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	a.dirty = false
	a.status = "已保存 " + filepath.Base(a.path)
	a.remember()
}

// saveBook writes the edited document back into the `.mdxb` it came from,
// keeping the book's envelope and matching chapters by title.
func (a *App) saveBook() {
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
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
	b.UpdateFromDocument(a.core.Doc)
	out, err := b.Bytes()
	if err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	if err := os.WriteFile(a.path, out, 0o644); err != nil {
		a.status = "保存失败：" + err.Error()
		return
	}
	a.dirty = false
	a.status = "已保存书籍 " + filepath.Base(a.path)
	a.remember()
}

// ExportHTML writes a self-contained HTML page next to the document.
func (a *App) ExportHTML() {
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
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
	page := htmlexport.Page(a.core.Doc, htmlexport.Options{Title: name})
	if err := os.WriteFile(p, []byte(page), 0o644); err != nil {
		a.status = "导出失败：" + err.Error()
		return
	}
	a.status = "已导出 " + filepath.Base(p)
}

// ExportBook writes the document as a `.mdxb` book.
func (a *App) ExportBook() {
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
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
	b := book.FromDocument(a.core.Doc, a.titleText())
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
	a.OpenFile(paths[0])
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
	a.root = paths[0]
	a.refreshFiles()
	a.status = "工作区 " + filepath.Base(a.root)
	a.remember()
}

// setSideTab switches the sidebar and remembers the choice.
func (a *App) setSideTab(i int) {
	if a.sideTab == i || (i != 0 && i != 1) {
		return
	}
	a.sideTab = i
	a.remember()
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
	a.setSideTab(1)
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
	a.setSideTab(1)
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
	a.syncAll()
	a.core.Checkpoint(editor.EditOther)
	a.core.Doc.Blocks = append(a.core.Doc.Blocks, blocks...)
	a.core.Generation++
	a.core.Dirty = true
	a.focus = blocks[len(blocks)-1].ID
	a.dirty = true
	a.ResetBuffers()
	a.status = "已插入模板 " + t.Name
}

func (a *App) refreshFiles() {
	a.files = a.files[:0]
	if a.root == "" {
		return
	}
	_ = filepath.WalkDir(a.root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if d.IsDir() {
			name := d.Name()
			if path != a.root && (strings.HasPrefix(name, ".") || name == "node_modules" || name == "dist" || name == "target") {
				return filepath.SkipDir
			}
			return nil
		}
		if convert.Detect(path) == convert.FormatMarkdown || convert.Detect(path) == convert.FormatMdxb {
			rel, err := filepath.Rel(a.root, path)
			if err == nil {
				a.files = append(a.files, rel)
			}
		}
		return nil
	})
	sort.Strings(a.files)
}

func (a *App) titleText() string { return strings.TrimSpace(*a.title) }

// DisplayName is the document's name in the window title and dialogs.
func (a *App) DisplayName() string {
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
		if p, ok := a.bufs[b.ID]; ok {
			*p = b.Text.Text
			return
		}
		s := b.Text.Text
		a.bufs[b.ID] = &s
	})
	for id := range a.bufs {
		if !seen[id] {
			delete(a.bufs, id)
		}
	}
	*a.title = a.core.Doc.Title.Text
}

// restoreSelection asks the text input of id to put its caret back, once.
func (a *App) restoreSelection(id doc.BlockID, start, end int) {
	a.pending[id] = [2]int{start, end}
}

func (a *App) buf(id doc.BlockID) *string {
	if s, ok := a.bufs[id]; ok {
		return s
	}
	s := ""
	a.bufs[id] = &s
	return &s
}

func (a *App) sync(b *doc.Block) {
	if b == nil || !b.Kind.HasText() {
		return
	}
	s := a.buf(b.ID)
	if *s == b.Text.Text {
		return
	}
	// One undo step per run of typing, as an editor is expected to have.
	a.core.Checkpoint(editor.EditInsert)
	old := b.Text.Text
	// Resync keeps the marks that still line up, so typing next to a bold word
	// does not flatten it.
	b.Text.Resync(*s)
	// A format toggled with nothing selected (Ctrl+B, Ctrl+E) applies to the
	// text typed next, as Typora does.
	if pm := a.core.PendingMk; pm != nil {
		if start, end, ok := typedSpan(old, *s); ok {
			applyRangeMarks(&b.Text, start, end, *pm)
			a.core.PendingMk = nil
		}
	}
	a.dirty = true
}

func (a *App) syncAll() {
	a.core.Doc.ForEachBlock(func(b *doc.Block) { a.sync(b) })
}

func (a *App) focused() *doc.Block {
	if b := a.core.Block(a.focus); b != nil {
		return b
	}
	if len(a.core.Doc.Blocks) > 0 {
		return a.core.Doc.Blocks[0]
	}
	return nil
}

func (a *App) markdownSource() string {
	a.syncAll()
	a.core.Doc.Title = doc.PlainRich(*a.title)
	return doc.Export(a.core.Doc, doc.Options{}).Text
}

func (a *App) setStatus(s string) { a.status = s }

// setMode switches the pane layout and remembers it.
func (a *App) setMode(m Mode) {
	a.mode = m
	a.remember()
}

func (a *App) bullet() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.ToggleList()
	a.ResetBuffers()
}

func (a *App) ordered() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.ToggleOrdered()
	a.ResetBuffers()
}

func (a *App) todo() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.ToggleTodo()
	a.ResetBuffers()
}

func (a *App) quote() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.ToggleQuote()
	a.ResetBuffers()
}

func (a *App) code() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.ToggleCode()
	a.ResetBuffers()
}

func (a *App) divider() {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	nb := doc.NewBlock(doc.DividerKind{}, doc.RichText{})
	a.core.InsertBlockAfter(b.ID, nb)
	after := doc.NewBlock(doc.TextKind{}, doc.RichText{})
	a.core.InsertBlockAfter(nb.ID, after)
	a.focus = after.ID
	a.focusReq = after.ID
	a.ResetBuffers()
	a.restoreSelection(after.ID, 0, 0)
}

// insertTable adds a 3×3 table after the focused block.
func (a *App) insertTable() {
	a.syncAll()
	t := &doc.Table{HeaderRows: 1}
	t.ColWidths = []float32{200, 200, 200}
	t.Rows = make([][]doc.TableCell, 3)
	for r := 0; r < 3; r++ {
		t.Rows[r] = make([]doc.TableCell, 3)
		for c := 0; c < 3; c++ {
			cell := doc.EmptyCell()
			cell.Blocks = []*doc.Block{doc.TextBlock("")}
			t.Rows[r][c] = cell
		}
	}
	t.Repair()
	a.insertBlock(doc.NewBlock(doc.TableKind{Table: t}, doc.RichText{}))
}

// insertImage copies a picture next to the document and inserts it.
func (a *App) insertImage() {
	paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
		Parent:  a.win,
		Title:   "插入图片",
		Filters: []mygo.FileFilter{{Name: "图片", MIMETypes: []string{"image/*"}}},
	})
	if err != nil || len(paths) == 0 {
		return
	}
	src := paths[0]
	rel := src
	if a.path != "" {
		assets := filepath.Join(filepath.Dir(a.path), "assets")
		if err := os.MkdirAll(assets, 0o755); err == nil {
			data, rerr := os.ReadFile(src)
			if rerr == nil {
				name := filepath.Base(src)
				if werr := os.WriteFile(filepath.Join(assets, name), data, 0o644); werr == nil {
					rel = "assets/" + name
				}
			}
		}
	}
	a.syncAll()
	a.insertBlock(doc.NewBlock(doc.ImageKind{SrcKind: "src", Src: rel}, doc.RichText{}))
}

// insertBlock adds a block after the focused one, then a paragraph to keep
// typing in.
func (a *App) insertBlock(b *doc.Block) {
	cur := a.focused()
	if cur == nil {
		a.core.Doc.Blocks = append(a.core.Doc.Blocks, b)
	} else {
		a.core.InsertBlockAfter(cur.ID, b)
	}
	after := doc.NewBlock(doc.TextKind{}, doc.RichText{})
	a.core.InsertBlockAfter(b.ID, after)
	a.focus = after.ID
	a.focusReq = after.ID
	a.ResetBuffers()
	a.restoreSelection(after.ID, 0, 0)
}

func (a *App) heading(level int) {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	a.core.Sel = editor.Caret(editor.Pos{Block: b.ID})
	a.core.SetHeading(level)
	a.ResetBuffers()
}

// setMark toggles a character format over the focused block's selection, or
// arms it for the next typed text when nothing is selected.
func (a *App) setMark(has func(*doc.Marks) bool, set func(*doc.Marks, bool)) {
	b := a.focused()
	if b == nil {
		return
	}
	a.syncAll()
	lo, hi := a.selectionOf(b)
	a.core.Sel = editor.Selection{
		Anchor: editor.Pos{Block: b.ID, Offset: lo},
		Head:   editor.Pos{Block: b.ID, Offset: hi},
	}
	a.core.ToggleMark(has, set)
	a.ResetBuffers()
	text := b.Text.Text
	a.restoreSelection(b.ID, doc.ByteToRune(text, lo), doc.ByteToRune(text, hi))
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

func (a *App) bold()      { a.setMark(func(m *doc.Marks) bool { return m.Bold }, func(m *doc.Marks, on bool) { m.Bold = on }) }
func (a *App) italic()    { a.setMark(func(m *doc.Marks) bool { return m.Italic }, func(m *doc.Marks, on bool) { m.Italic = on }) }
func (a *App) underline() { a.setMark(func(m *doc.Marks) bool { return m.Underline }, func(m *doc.Marks, on bool) { m.Underline = on }) }
func (a *App) strike()    { a.setMark(func(m *doc.Marks) bool { return m.Strike }, func(m *doc.Marks, on bool) { m.Strike = on }) }
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
	if a.core.Undo() {
		a.ResetBuffers()
	}
}

func (a *App) redo() {
	if a.core.Redo() {
		a.ResetBuffers()
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

// Run starts the desktop app.
func Run(path string) error {
	app := mygo.App
	app.SetName("MarkdownX")
	st := New(path)
	// Files opened from Explorer/Finder, the Dock, a jump list or a second
	// launch arrive here rather than on the command line.
	app.OnOpenFile(func(p string) { st.OpenFile(p) })
	app.WhenReady(func() {
		if path == "" {
			st.restore("")
		}
		app.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
			{Role: mygo.RoleAppMenu},
			{Label: "文件", Submenu: []*mygo.MenuItem{
				{Label: "新建", Accelerator: "CmdOrCtrl+N", Click: func(*mygo.MenuItem, *mygo.Window) { st.NewDocument() }},
				{Label: "打开…", Accelerator: "CmdOrCtrl+O", Click: func(*mygo.MenuItem, *mygo.Window) { st.ChooseFile() }},
				{Label: "打开文件夹…", Accelerator: "CmdOrCtrl+Shift+O", Click: func(*mygo.MenuItem, *mygo.Window) { st.ChooseFolder() }},
				mygo.Separator(),
				{Label: "保存", Accelerator: "CmdOrCtrl+S", Click: func(*mygo.MenuItem, *mygo.Window) { st.Save() }},
				{Label: "密码保存…", Accelerator: "CmdOrCtrl+Shift+S", Click: func(*mygo.MenuItem, *mygo.Window) { st.EncryptSave() }},
				{Label: "导出 HTML…", Accelerator: "CmdOrCtrl+Shift+E", Click: func(*mygo.MenuItem, *mygo.Window) { st.ExportHTML() }},
				{Label: "导出书籍 (.mdxb)…", Click: func(*mygo.MenuItem, *mygo.Window) { st.ExportBook() }},
				{Label: "存为模板…", Click: func(*mygo.MenuItem, *mygo.Window) { st.SaveAsTemplate() }},
				mygo.Separator(),
				{Role: mygo.RoleClose},
			}},
			{Label: "视图", Submenu: []*mygo.MenuItem{
				{Label: "仅编辑", Accelerator: "CmdOrCtrl+Alt+1", Click: func(*mygo.MenuItem, *mygo.Window) { st.setMode(ModeEdit) }},
				{Label: "分栏", Accelerator: "CmdOrCtrl+Alt+2", Click: func(*mygo.MenuItem, *mygo.Window) { st.setMode(ModeSplit) }},
				{Label: "仅预览", Accelerator: "CmdOrCtrl+Alt+3", Click: func(*mygo.MenuItem, *mygo.Window) { st.setMode(ModePreview) }},
			}},
			{Label: "格式", Submenu: []*mygo.MenuItem{
				{Label: "标题 1", Accelerator: "CmdOrCtrl+1", Click: func(*mygo.MenuItem, *mygo.Window) { st.heading(1) }},
				{Label: "标题 2", Accelerator: "CmdOrCtrl+2", Click: func(*mygo.MenuItem, *mygo.Window) { st.heading(2) }},
				{Label: "标题 3", Accelerator: "CmdOrCtrl+3", Click: func(*mygo.MenuItem, *mygo.Window) { st.heading(3) }},
				{Label: "正文", Accelerator: "CmdOrCtrl+0", Click: func(*mygo.MenuItem, *mygo.Window) { st.heading(0) }},
				mygo.Separator(),
				{Label: "加粗", Accelerator: "CmdOrCtrl+B", Click: func(*mygo.MenuItem, *mygo.Window) { st.bold() }},
				{Label: "斜体", Accelerator: "CmdOrCtrl+I", Click: func(*mygo.MenuItem, *mygo.Window) { st.italic() }},
				{Label: "下划线", Accelerator: "CmdOrCtrl+U", Click: func(*mygo.MenuItem, *mygo.Window) { st.underline() }},
				{Label: "删除线", Click: func(*mygo.MenuItem, *mygo.Window) { st.strike() }},
				{Label: "行内代码", Accelerator: "CmdOrCtrl+E", Click: func(*mygo.MenuItem, *mygo.Window) { st.inlineCode() }},
				{Label: "链接…", Accelerator: "CmdOrCtrl+K", Click: func(*mygo.MenuItem, *mygo.Window) { st.openLink() }},
				mygo.Separator(),
				{Label: "项目符号", Accelerator: "CmdOrCtrl+Shift+[", Click: func(*mygo.MenuItem, *mygo.Window) { st.bullet() }},
				{Label: "编号列表", Accelerator: "CmdOrCtrl+Shift+]", Click: func(*mygo.MenuItem, *mygo.Window) { st.ordered() }},
				{Label: "待办列表", Accelerator: "CmdOrCtrl+Shift+C", Click: func(*mygo.MenuItem, *mygo.Window) { st.todo() }},
				{Label: "引用", Accelerator: "CmdOrCtrl+Shift+Q", Click: func(*mygo.MenuItem, *mygo.Window) { st.quote() }},
				{Label: "代码块", Accelerator: "CmdOrCtrl+Shift+K", Click: func(*mygo.MenuItem, *mygo.Window) { st.code() }},
				{Label: "表格", Accelerator: "CmdOrCtrl+T", Click: func(*mygo.MenuItem, *mygo.Window) { st.insertTable() }},
				{Label: "分割线", Click: func(*mygo.MenuItem, *mygo.Window) { st.divider() }},
				mygo.Separator(),
				{Label: "下方插入行", Accelerator: "CmdOrCtrl+Shift+Enter", Click: func(*mygo.MenuItem, *mygo.Window) { st.insertBelow() }},
				{Label: "查找…", Accelerator: "CmdOrCtrl+F", Click: func(*mygo.MenuItem, *mygo.Window) { st.finding = true; st.findFocus = true; st.findKey = "" }},
				{Label: "替换…", Accelerator: "CmdOrCtrl+H", Click: func(*mygo.MenuItem, *mygo.Window) { st.finding = true; st.findFocus = true; st.findKey = "" }},
			}},
			{Role: mygo.RoleEditMenu},
			{Role: mygo.RoleWindowMenu},
		}))
		w := mygo.NewWindow(mygo.WindowOptions{
			Title:     "MarkdownX",
			Width:     1120,
			Height:    760,
			MinWidth:  760,
			MinHeight: 420,
			Content:   ui.View(st.View),
		})
		st.win = w
		st.winID = w.ID()
		st.SyncTitle()
		// Keep unsaved work from disappearing when the window closes.
		w.OnClose(func(e *mygo.CloseEvent) {
			if !st.dirty {
				return
			}
			res, err := mygo.Dialog.Message(mygo.MessageOptions{
				Parent:        w,
				Type:          mygo.MessageWarning,
				Title:         "未保存的修改",
				Message:       "关闭前要保存 " + st.DisplayName() + " 吗？",
				Buttons:       []string{"保存", "不保存", "取消"},
				DefaultButton: 0,
				CancelButton:  2,
			})
			if err != nil {
				e.PreventDefault()
				return
			}
			switch res.Button {
			case 0:
				st.Save()
				if st.dirty {
					e.PreventDefault()
				}
			case 2:
				e.PreventDefault()
			}
		})
	})
	return app.Run()
}

// NewDocument starts an empty document.
func (a *App) NewDocument() {
	if !a.confirmDiscard() {
		return
	}
	a.core = editor.New(doc.NewDocument())
	a.path = ""
	a.dirty = false
	a.status = ""
	a.ResetBuffers()
}

// confirmDiscard asks what to do about unsaved changes before replacing the
// open document. It reports whether the caller may continue.
func (a *App) confirmDiscard() bool {
	if !a.dirty {
		return true
	}
	res, err := mygo.Dialog.Message(mygo.MessageOptions{
		Parent:        a.win,
		Type:          mygo.MessageWarning,
		Title:         "未保存的修改",
		Message:       "要先保存 " + a.DisplayName() + " 吗？",
		Buttons:       []string{"保存", "不保存", "取消"},
		DefaultButton: 0,
		CancelButton:  2,
	})
	if err != nil {
		return false
	}
	switch res.Button {
	case 0:
		a.Save()
		return !a.dirty
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
	a.core.Doc.Title = doc.PlainRich(*a.title)
	src := doc.Export(a.core.Doc, doc.Options{}).Text
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
	a.dirty = false
	a.status = "已加密保存 " + filepath.Base(target)
	a.encPw1 = ""
	a.encPw2 = ""
}
