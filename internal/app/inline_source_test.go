package app

import (
	"image"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/editor"
)

func TestInlineSourceCaretEditSaveAndUndo(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "inline.md")
	const original = "开头 __粗体词__ 与 [链接](#章节 \"说明\") 收尾\n\n# 章节\n"
	if err := os.WriteFile(path, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: len("开头 粗")}))
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	p := a.bufs[b.ID]
	if a.inlineEdit == nil || p.native.Text().String() != "开头 __粗体词__ 与 链接 收尾" || a.dirty {
		t.Fatal("styled span did not reveal its original markup")
	}
	tr.Type("新")
	want := strings.Replace(original, "粗体词", "粗新体词", 1)
	if got := a.markdownSource(); got != want {
		t.Fatalf("edited inline Markdown = %q, want %q", got, want)
	}
	tr.Key(ui.Cmd, ui.KeyS)
	if saved, err := os.ReadFile(path); err != nil || string(saved) != want {
		t.Fatalf("save while editing a span: %q (%v)", saved, err)
	}
	tr.Key(0, ui.KeyEscape)
	if a.inlineEdit != nil || p.native.Text().String() != "开头 粗新体词 与 链接 收尾" {
		t.Fatal("Escape did not return to rich text")
	}
	tr.Key(ui.Cmd, ui.KeyZ)
	if got := a.markdownSource(); got != original {
		t.Fatalf("undo = %q", got)
	}
}

func clickInBlock(t *testing.T, tr *ui.Tester, p *blockBuffer, byteAt int, mods ui.Modifiers) {
	t.Helper()
	shape := p.layout(0, p.bounds.W)
	if shape == nil {
		t.Fatal("no block layout")
	}
	caret := shape.Caret(utf16Index(p.visible, byteAt))
	tr.ClickAtWith(mods, p.bounds.X+caret.X+0.1, p.bounds.Y+caret.Y+caret.H/2)
}

func TestInlineLinkUsesClickToEditAndCommandClickToFollow(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("看这里 [链接字样](#重复-1) 收尾\n\n# 重复\n\n# 重复\n")
	b := a.core.Doc.Blocks[0]
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	p := a.bufs[b.ID]
	clickInBlock(t, tr, p, len("看这里 链接"), 0)
	if a.inlineEdit == nil || !strings.Contains(p.visible, "[链接字样](#重复-1)") {
		t.Fatal("click did not expand the link")
	}
	tr.Key(0, ui.KeyEscape)
	tr.Advance(time.Second)
	clickInBlock(t, tr, p, len("看这里 链接"), ui.Cmd)
	if a.focus != a.core.Doc.Blocks[2].ID || a.inlineEdit != nil {
		t.Fatalf("command-click focus=%d, pending=%q, status=%q", a.focus, a.pendingLink, a.status)
	}
}

func TestInlineLinkOpensRelativeDocumentAtItsHeading(t *testing.T) {
	dir := isolateConfig(t)
	path := filepath.Join(dir, "index.md")
	notes := filepath.Join(dir, "工作 (二).md")
	for name, content := range map[string]string{
		path:  "阅读 [项目记录](工作%20%28二%29#%E8%AE%B0%E5%BD%95) 再回来\n",
		notes: "引言\n\n# 记录\n\n内容\n",
	} {
		if err := os.WriteFile(name, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	a := New(path)
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	clickInBlock(t, tr, a.bufs[a.core.Doc.Blocks[0].ID], len("阅读 项目"), ui.Cmd)
	if !samePath(a.path, notes) || len(a.core.Doc.Blocks) != 3 || a.focus != a.core.Doc.Blocks[1].ID {
		t.Fatalf("relative link did not navigate to the heading in %q: %q", a.path, a.status)
	}
}

func TestInlineFormulaOpensNativeObjectSource(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("公式 $x^2$ 后缀")
	b := a.core.Doc.Blocks[0]
	tr := ui.NewTester(a.View, 900, 680)
	tr.Frame()
	p := a.bufs[b.ID]
	shape := p.layout(0, p.bounds.W)
	if shape == nil || len(shape.Objects()) != 1 {
		t.Fatal("formula did not render an inline object")
	}
	r := shape.Objects()[0].Rect
	tr.ClickAt(p.bounds.X+r.X+r.W/2, p.bounds.Y+r.Y+r.H/2)
	if a.inlineEdit == nil || p.visible != "公式 $x^2$ 后缀" {
		t.Fatal("clicking the formula did not expose its source")
	}
	tr.Type("2")
	tr.Key(0, ui.KeyEscape)
	markup := a.markdownSource()
	if markup != "公式 $2x^2$ 后缀\n" || !strings.Contains(b.Text.Text, doc.Obj) {
		t.Fatalf("formula edit = %q", markup)
	}
}

func TestEditExistingLinkAndPasteURLInTheSource(t *testing.T) {
	isolateConfig(t)
	a := newTestApp(`前缀 [阅读](<draft (1).md> "说明 (保留)") 后缀`)
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: len("前缀 阅")}))
	tr := ui.NewTester(a.View, 940, 650)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyK)
	tr.Key(ui.Cmd|ui.Shift, ui.KeyC)
	if got := tr.Clipboard(); got != "draft (1).md" {
		t.Fatalf("edited URL selection = %q", got)
	}
	tr.SetClipboard("稿件 (2).md")
	tr.Key(ui.Cmd|ui.Shift, ui.KeyV)
	tr.Key(ui.Cmd, ui.KeySlash)
	if a.source == nil || !strings.Contains(a.source.text(), `[阅读](<稿件 (2).md> "说明 (保留)")`) {
		t.Fatalf("plain paste changed the wrong part of the link: %q", a.markdownSource())
	}
	if offset := a.source.edit.Selection().Head; a.source.text()[:offset] != "前缀 [阅读](<稿件 (2).md" {
		t.Fatalf("source-mode caret offset = %d", offset)
	}
}

func TestSourceModeKeepsCaretInTheSecondIdenticalLink(t *testing.T) {
	isolateConfig(t)
	const markup = `[说明](readme.md) 和 [说明](readme.md)`
	a := newTestApp(markup)
	b := a.core.Doc.Blocks[0]
	a.focusAt(editor.Caret(editor.Pos{Block: b.ID, Offset: len("说明 和 说")}))
	tr := ui.NewTester(a.View, 940, 650)
	tr.Frame()
	tr.Key(ui.Cmd, ui.KeyK)
	tr.Key(ui.Cmd, ui.KeySlash)
	if a.source == nil || a.source.edit.Selection().Head != strings.LastIndex(markup, "readme.md")+len("readme.md") {
		t.Fatal("toggling to source while editing the second link did not keep the caret there")
	}
}

func TestDragFromExpandedInlineSourceSelectsDocumentText(t *testing.T) {
	isolateConfig(t)
	a := newTestApp("甲 __乙丙丁__ 戊\n\n后文")
	first, last := a.core.Doc.Blocks[0], a.core.Doc.Blocks[1]
	start := editor.Pos{Block: first.ID, Offset: len("甲 乙")}
	end := editor.Pos{Block: last.ID, Offset: len("后")}
	a.focusAt(editor.Caret(start))
	tr := ui.NewTester(a.View, 940, 650)
	tr.Frame()
	if a.inlineEdit == nil {
		t.Fatal("expected the inline source to be expanded")
	}
	point := func(b *doc.Block, offset int) ui.Point {
		p := a.bufs[b.ID]
		shape := p.layout(0, p.bounds.W)
		if r := a.inlineFor(b); r != nil {
			offset = r.viewOffset(offset)
		}
		caret := shape.Caret(utf16Index(p.visible, offset))
		return ui.Point{X: p.bounds.X + caret.X + 0.15, Y: p.bounds.Y + caret.Y + caret.H/2}
	}
	from, to := point(first, start.Offset), point(last, end.Offset)
	tr.Press(from.X, from.Y)
	tr.Move(to.X, to.Y)
	p := a.bufs[first.ID]
	lo, hi := p.native.Selection()
	if a.inlineEdit != nil || min(lo, hi) != doc.ByteToRune(first.Text.Text, start.Offset) {
		t.Fatalf("native drag range %d..%d was not projected onto the formatted text %q", lo, hi, p.visible)
	}
	tr.Release(to.X, to.Y)
	if sel := a.core.Sel; sel.Anchor != start || sel.Head != end {
		t.Fatalf("drag from inline source selected %+v", sel)
	}
}

func TestBlockImageSourceKeepsPathTitleAndUndo(t *testing.T) {
	dir := isolateConfig(t)
	name := "draft (1).png"
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	if err := png.Encode(f, image.NewRGBA(image.Rect(0, 0, 48, 48))); err != nil {
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "source.md")
	const original = "# 资料\n\n![展示](<draft (1).png> \"备注 (一)\")\n"
	if err := os.WriteFile(path, []byte(original), 0o600); err != nil {
		t.Fatal(err)
	}
	a := New(path)
	tr := ui.NewTester(a.View, 900, 650)
	tr.Frame()
	if err := tr.Click("编辑图片 Markdown 源码"); err != nil {
		t.Fatal(err)
	}
	if a.imageEdit == nil || a.imageEdit.draft != `![展示](<draft (1).png> "备注 (一)")` || !tr.Focused("图片 Markdown 源码") {
		t.Fatal("click did not open the original image Markdown")
	}
	tr.Key(ui.Cmd, ui.KeyA)
	tr.Type(`![新图注](<draft (1).png> "备注 (二)")`)
	tr.Key(0, ui.KeyEnter)
	if a.imageEdit != nil || !strings.Contains(a.markdownSource(), `![新图注](<draft (1).png> "备注 (二)")`) {
		t.Fatalf("image edit wrote %q", a.markdownSource())
	}
	img, ok := a.core.Doc.Blocks[1].Kind.(doc.ImageKind)
	if !ok || img.Src != name {
		t.Fatalf("image path was read as %q", img.Src)
	}
	a.undo()
	if got := a.markdownSource(); got != original {
		t.Fatalf("undo image source = %q", got)
	}
}
